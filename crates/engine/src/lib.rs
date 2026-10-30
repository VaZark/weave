//! The one engine API. Frontends call this and never touch storage directly.

mod autoplay;
mod player;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use weave_core::{
    Backend, CapabilityDescriptor, Error, FilterSpec, Page, Playlist, PlaylistKind, Result, Rng,
    ScannedTrack, SearchResult, Track,
};
use weave_local::LocalBackend;
use weave_storage::Storage;

pub use player::SessionView;
pub use weave_storage::ImportStats;

pub struct BackendInfo {
    pub id: String,
    pub capabilities: Vec<CapabilityDescriptor>,
}

pub struct Library {
    storage: Storage,
    local: LocalBackend,
    /// Shuffle randomness; seeded from the clock, replaceable for tests.
    rng: Rng,
    /// Unix milliseconds; replaceable for tests (sleep timer).
    clock: Box<dyn Fn() -> i64>,
}

fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Library {
    pub fn open(db_path: &Path) -> Result<Self> {
        Ok(Self::with_storage(Storage::open(db_path)?))
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self::with_storage(Storage::open_in_memory()?))
    }

    fn with_storage(storage: Storage) -> Self {
        Self {
            storage,
            local: LocalBackend,
            rng: Rng::new(system_now_ms() as u64),
            clock: Box::new(system_now_ms),
        }
    }

    /// Fix the shuffle seed and the clock (unix ms). For tests and reproducible runs.
    pub fn with_determinism(mut self, seed: u64, clock: impl Fn() -> i64 + 'static) -> Self {
        self.rng = Rng::new(seed);
        self.clock = Box::new(clock);
        self
    }

    // --- library -----------------------------------------------------------

    pub fn import(&mut self, path: &Path) -> Result<ImportStats> {
        let scanned = self.local.scan(path)?;
        self.storage.import_tracks(self.local.id(), &scanned)
    }

    /// Index tracks reported by any backend (also used for fixtures and seed data).
    /// Already-known `(backend_id, external_id)` pairs are skipped.
    pub fn import_scanned(
        &mut self,
        backend_id: &str,
        tracks: &[ScannedTrack],
    ) -> Result<ImportStats> {
        self.storage.import_tracks(backend_id, tracks)
    }

    pub fn search(&self, filter: &FilterSpec, page: Page) -> Result<SearchResult> {
        self.storage.search(filter, page)
    }

    pub fn track(&self, id: &str) -> Result<Track> {
        self.storage.track(id)
    }

    // --- tags --------------------------------------------------------------

    pub fn add_tags(&mut self, track_id: &str, tags: &[String]) -> Result<usize> {
        self.storage.add_tags(track_id, tags)
    }

    pub fn remove_tags(&mut self, track_id: &str, tags: &[String]) -> Result<usize> {
        self.storage.remove_tags(track_id, tags)
    }

    pub fn tags(&self, track_id: &str) -> Result<Vec<String>> {
        self.storage.tags(track_id)
    }

    // --- favorites and ratings -------------------------------------------

    /// Returns how many tracks changed. An unknown ID changes nothing.
    pub fn set_favorite(&mut self, track_ids: &[String], favorite: bool) -> Result<usize> {
        self.storage.set_favorite(track_ids, favorite)
    }

    /// 1 to 5 stars; `None` clears the rating.
    pub fn set_rating(&mut self, track_id: &str, rating: Option<u8>) -> Result<()> {
        self.storage.set_rating(track_id, rating)
    }

    // --- playlists ---------------------------------------------------------

    /// Dynamic playlist: saves the filter (with its sort and limit), not the tracks.
    pub fn create_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
        filter: FilterSpec,
    ) -> Result<Playlist> {
        self.storage.create_playlist(name, description, &filter)
    }

    /// Manual playlist: an explicit, ordered list filled with `playlist_add`.
    pub fn create_manual_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
    ) -> Result<Playlist> {
        self.storage.create_manual_playlist(name, description)
    }

    pub fn playlist_add(&mut self, name: &str, track_ids: &[String]) -> Result<usize> {
        self.storage.playlist_add(name, track_ids)
    }

    pub fn playlist_remove(&mut self, name: &str, track_ids: &[String]) -> Result<usize> {
        self.storage.playlist_remove(name, track_ids)
    }

    /// Move a track of a manual playlist to another place in its play order (1-based positions).
    pub fn playlist_move(&mut self, name: &str, from: usize, to: usize) -> Result<()> {
        let index = |p: usize| {
            p.checked_sub(1)
                .ok_or_else(|| Error::InvalidInput("playlist positions start at 1".into()))
        };
        self.storage.playlist_move(name, index(from)?, index(to)?)
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        self.storage.playlists()
    }

    pub fn playlist(&self, name: &str) -> Result<Playlist> {
        self.storage.playlist(name)
    }

    /// A playlist's tracks: manual ones in saved order, dynamic ones by evaluating
    /// the filter against the current library (sorted and limited as saved).
    pub fn playlist_tracks(&self, name: &str, page: Page) -> Result<(Playlist, SearchResult)> {
        let p = self.storage.playlist(name)?;
        let result = match p.kind {
            PlaylistKind::Dynamic => self.storage.search(&p.filter, page)?,
            PlaylistKind::Manual => self.storage.manual_playlist_tracks(&p.id, page)?,
        };
        Ok((p, result))
    }

    /// Change the specified filters; retain the others. `description`: `None` keeps
    /// the current one, `Some("")` clears it. Manual playlists only take a description.
    pub fn update_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
        update: FilterSpec,
    ) -> Result<Playlist> {
        let mut p = self.storage.playlist(name)?;
        if p.kind == PlaylistKind::Manual && update.clone().normalized() != FilterSpec::default() {
            return Err(Error::InvalidInput(format!(
                "playlist \"{}\" is manual and has no filters; use playlist add/remove",
                p.name
            )));
        }
        let merged = p.filter.clone().merged(update).normalized();
        merged.validate()?;
        if let Some(d) = description {
            p.description = Some(d.trim().to_string()).filter(|d| !d.is_empty());
        }
        self.storage
            .update_playlist(&p.id, p.description.as_deref(), &merged)?;
        p.filter = merged;
        Ok(p)
    }

    // --- backends ----------------------------------------------------------

    pub fn backends(&self) -> Vec<BackendInfo> {
        vec![BackendInfo {
            id: self.local.id().into(),
            capabilities: self.local.capabilities(),
        }]
    }

    pub fn backend(&self, id: &str) -> Result<BackendInfo> {
        self.backends()
            .into_iter()
            .find(|b| b.id == id)
            .ok_or_else(|| Error::NotFound(format!("backend \"{id}\"")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_playlist_retains_other_filters() {
        let mut lib = Library::open_in_memory().unwrap();
        let base = FilterSpec {
            tag_all: vec!["bachata".into()],
            ..Default::default()
        };
        lib.create_playlist("P", None, base).unwrap();
        let p = lib
            .update_playlist(
                "p",
                None,
                FilterSpec {
                    bpm_min: Some(90.0),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p.filter.tag_all, ["bachata"]);
        assert_eq!(lib.playlist("P").unwrap().filter.bpm_min, Some(90.0));
    }

    #[test]
    fn update_rejects_invalid_merge() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_playlist(
            "P",
            None,
            FilterSpec {
                bpm_max: Some(100.0),
                ..Default::default()
            },
        )
        .unwrap();
        let r = lib.update_playlist(
            "P",
            None,
            FilterSpec {
                bpm_min: Some(120.0),
                ..Default::default()
            },
        );
        assert!(matches!(r, Err(Error::InvalidFilter(_))));
        assert_eq!(lib.playlist("P").unwrap().filter.bpm_min, None);
    }

    #[test]
    fn update_playlist_description() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_playlist("P", Some("first"), FilterSpec::default())
            .unwrap();
        let p = lib
            .update_playlist("P", None, FilterSpec::default())
            .unwrap();
        assert_eq!(p.description.as_deref(), Some("first"));
        let p = lib
            .update_playlist("P", Some("second"), FilterSpec::default())
            .unwrap();
        assert_eq!(lib.playlist("P").unwrap().description, p.description);
        let p = lib
            .update_playlist("P", Some(""), FilterSpec::default())
            .unwrap();
        assert_eq!(p.description, None);
        assert_eq!(lib.playlist("P").unwrap().description, None);
    }

    #[test]
    fn import_scanned_is_idempotent() {
        let mut lib = Library::open_in_memory().unwrap();
        let t = ScannedTrack {
            external_id: "x".into(),
            metadata: weave_core::Metadata {
                title: "T".into(),
                ..Default::default()
            },
        };
        assert_eq!(
            lib.import_scanned("fake", std::slice::from_ref(&t))
                .unwrap()
                .added,
            1
        );
        assert_eq!(lib.import_scanned("fake", &[t]).unwrap().skipped, 1);
    }

    fn lib_with(titles: &[&str]) -> Library {
        let mut lib = Library::open_in_memory().unwrap();
        let tracks: Vec<_> = titles
            .iter()
            .map(|t| ScannedTrack {
                external_id: (*t).into(),
                metadata: weave_core::Metadata {
                    title: (*t).into(),
                    artist: Some("x".into()),
                    ..Default::default()
                },
            })
            .collect();
        lib.import_scanned("fake", &tracks).unwrap();
        lib
    }

    #[test]
    fn manual_playlists_take_a_description_but_no_filters() {
        let mut lib = lib_with(&["a"]);
        lib.create_manual_playlist("Mix", None).unwrap();
        let p = lib
            .update_playlist("mix", Some("road"), FilterSpec::default())
            .unwrap();
        assert_eq!(
            (p.kind, p.description.as_deref()),
            (PlaylistKind::Manual, Some("road"))
        );
        let r = lib.update_playlist(
            "mix",
            None,
            FilterSpec {
                bpm_min: Some(90.0),
                ..Default::default()
            },
        );
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn playlist_tracks_follow_the_kind() {
        let mut lib = lib_with(&["a", "b", "c"]);
        lib.create_manual_playlist("Manual", None).unwrap();
        lib.playlist_add("manual", &["x-c-fake".into(), "x-a-fake".into()])
            .unwrap();
        lib.create_playlist("Dynamic", None, FilterSpec::default())
            .unwrap();
        let titles = |lib: &Library, name: &str| -> Vec<String> {
            let (_, r) = lib.playlist_tracks(name, Page::ALL).unwrap();
            r.tracks.into_iter().map(|t| t.metadata.title).collect()
        };
        assert_eq!(titles(&lib, "manual"), ["c", "a"]);
        assert_eq!(titles(&lib, "dynamic"), ["a", "b", "c"]);
        // dynamic membership is live
        lib.import_scanned(
            "fake",
            &[ScannedTrack {
                external_id: "d".into(),
                metadata: weave_core::Metadata {
                    title: "d".into(),
                    artist: Some("x".into()),
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        assert_eq!(titles(&lib, "dynamic").len(), 4);
        assert_eq!(titles(&lib, "manual").len(), 2);
    }

    #[test]
    fn a_manual_playlists_play_order_can_be_changed_and_playing_follows_it() {
        let mut lib = lib_with(&["a", "b", "c"]);
        lib.create_manual_playlist("Mix", None).unwrap();
        lib.playlist_add(
            "mix",
            &["x-a-fake".into(), "x-b-fake".into(), "x-c-fake".into()],
        )
        .unwrap();
        lib.playlist_move("mix", 3, 1).unwrap();
        let titles = |lib: &Library| -> Vec<String> {
            let (_, r) = lib.playlist_tracks("mix", Page::ALL).unwrap();
            r.tracks.into_iter().map(|t| t.metadata.title).collect()
        };
        assert_eq!(titles(&lib), ["c", "a", "b"]);

        lib.play_playlist("mix", false).unwrap();
        let queued: Vec<_> = lib
            .session()
            .unwrap()
            .tracks
            .into_iter()
            .map(|t| t.metadata.title)
            .collect();
        assert_eq!(
            queued,
            ["c", "a", "b"],
            "the queue starts in the playlist's order"
        );

        assert!(
            matches!(lib.playlist_move("mix", 0, 1), Err(Error::InvalidInput(_))),
            "positions start at 1"
        );
        assert!(matches!(
            lib.playlist_move("mix", 1, 4),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(titles(&lib), ["c", "a", "b"], "failed moves change nothing");
    }

    #[test]
    fn backend_ids_are_slugs() {
        let lib = Library::open_in_memory().unwrap();
        for b in lib.backends() {
            assert_eq!(
                weave_core::slugify(&b.id),
                b.id,
                "backend id must be a slug"
            );
        }
    }

    #[test]
    fn unknown_backend() {
        let lib = Library::open_in_memory().unwrap();
        assert!(lib.backend("local").is_ok());
        assert!(matches!(lib.backend("spotify"), Err(Error::NotFound(_))));
    }
}
