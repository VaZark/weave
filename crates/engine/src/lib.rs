//! The one engine API. Frontends call this and never touch storage directly.

mod analytics;
mod autoplay;
mod collection_order;
mod discovery;
mod player;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use weave_player_core::{
    AlbumEntry, AlbumProfile, ArtistEntry, ArtistProfile, ChildPolicy, DedupMark, Dependent,
    EntityKind, EntityRef, Error, FilterSpec, Instance, MergeStats, Page, PlayStat, Playlist,
    PlaylistKind, Result, Rng, ScannedTrack, SearchResult, ShufflePref, SimilarPair, Tag, Track,
    TrackDetail,
};
use weave_player_local::LocalFiles;
use weave_player_storage::Storage;

pub use player::SessionView;
pub use weave_player_storage::ImportStats;

pub struct Library {
    storage: Storage,
    local: LocalFiles,
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
            local: LocalFiles,
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
        self.storage.import_tracks(&scanned)
    }

    /// Import mixed files and folders together, returning partial scan failures explicitly.
    pub fn import_paths(
        &mut self,
        paths: &[std::path::PathBuf],
    ) -> Result<weave_player_core::ImportReport> {
        let (tracks, mut report) = self.local.scan_paths(paths);
        let stats = self.storage.import_tracks(&tracks)?;
        report.added = stats.added;
        report.skipped = stats.skipped;
        Ok(report)
    }

    /// Index tracks that were already scanned (also used for fixtures and seed data).
    /// Already-known file paths are skipped.
    pub fn import_scanned(&mut self, tracks: &[ScannedTrack]) -> Result<ImportStats> {
        self.storage.import_tracks(tracks)
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

    /// A track's tags.
    pub fn tags(&self, track_id: &str) -> Result<Vec<String>> {
        self.storage.tags(track_id)
    }

    /// Create a tag, optionally under an existing parent.
    pub fn create_tag(&mut self, name: &str, parent: Option<&str>) -> Result<Tag> {
        self.storage.create_tag(name, parent)
    }

    pub fn tag(&self, name: &str) -> Result<Tag> {
        self.storage.tag(name)
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        self.storage.list_tags()
    }

    /// Set or clear (`None`) a tag's parent; self-parenting and cycles are rejected.
    pub fn set_tag_parent(&mut self, name: &str, parent: Option<&str>) -> Result<Tag> {
        self.storage.set_tag_parent(name, parent)
    }

    /// Set a tag's icon (a key or path) and colour (`#rrggbb`); `None` clears that part.
    pub fn set_tag_style(
        &mut self,
        name: &str,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<Tag> {
        self.storage.set_tag_style(name, icon, color)
    }

    /// Delete a tag, detaching or deleting its children. Returns how many tags were deleted.
    pub fn delete_tag(&mut self, name: &str, children: ChildPolicy) -> Result<usize> {
        self.storage.delete_tag(name, children)
    }

    /// Add tags to a track, artist, album or playlist.
    pub fn add_tags_to(&mut self, kind: EntityKind, id: &str, tags: &[String]) -> Result<usize> {
        self.storage.add_tags_to(kind, id, tags)
    }

    pub fn remove_tags_from(
        &mut self,
        kind: EntityKind,
        id: &str,
        tags: &[String],
    ) -> Result<usize> {
        self.storage.remove_tags_from(kind, id, tags)
    }

    /// The tags on a track, artist, album or playlist.
    pub fn tags_of(&self, kind: EntityKind, id: &str) -> Result<Vec<String>> {
        self.storage.tags_of(kind, id)
    }

    // --- tracks, artists and albums in depth -------------------------------

    /// Everything about one track: metadata, tags, rating, file, the artist and album it is
    /// linked to, the playlists it is in and where it sits in the queue.
    pub fn track_detail(&self, id: &str) -> Result<TrackDetail> {
        let track = self.storage.track(id)?;
        let (added_at, artist, album) = self.storage.track_links(&track.id.0)?;
        let featured = self.storage.track_featured(&track.id.0)?;
        let mut playlists = Vec::new();
        for p in self.storage.playlists()? {
            let (_, found) = self.playlist_tracks(&p.id, Page::ALL)?;
            if found.tracks.iter().any(|t| t.id == track.id) {
                playlists.push(EntityRef {
                    id: p.id,
                    name: p.name,
                });
            }
        }
        let queue_positions = self
            .storage
            .load_queue()?
            .items
            .iter()
            .enumerate()
            .filter(|(_, t)| **t == track.id)
            .map(|(i, _)| i + 1)
            .collect();
        Ok(TrackDetail {
            paths: self.storage.source_paths(&track.id.0)?,
            added_at,
            artist,
            album,
            featured,
            playlists,
            queue_positions,
            track,
        })
    }

    /// Artists by name; the tag conditions of `filter` apply to the tags on artists.
    pub fn artists(&self, filter: &FilterSpec) -> Result<Vec<ArtistEntry>> {
        self.storage.artists(filter)
    }

    pub fn artist(&self, id: &str) -> Result<ArtistProfile> {
        self.storage.artist(id)
    }

    /// Albums, optionally one artist's; the tag conditions of `filter` apply to the tags on albums.
    pub fn albums(&self, filter: &FilterSpec, artist: Option<&str>) -> Result<Vec<AlbumEntry>> {
        self.storage.albums(filter, artist)
    }

    pub fn album(&self, id: &str) -> Result<AlbumProfile> {
        self.storage.album(id)
    }

    /// Link an album to an artist profile, or unlink it with `None`.
    pub fn link_album(&mut self, album: &str, artist: Option<&str>) -> Result<AlbumProfile> {
        self.storage.link_album(album, artist)
    }

    /// Change an artist's public ID.
    pub fn set_artist_slug(&mut self, artist: &str, slug: &str) -> Result<ArtistProfile> {
        self.storage.set_artist_slug(artist, slug)
    }

    /// Split an artist: a new artist (same name unless `name` says otherwise) takes the listed
    /// tracks and albums. For people who share a name.
    pub fn split_artist(
        &mut self,
        artist: &str,
        tracks: &[String],
        albums: &[String],
        name: Option<&str>,
        slug: Option<&str>,
    ) -> Result<ArtistProfile> {
        self.storage
            .split_artist(artist, tracks, albums, name, slug)
    }

    // --- duplicates --------------------------------------------------------

    /// A point to compare against later, taken before an import.
    pub fn dedup_mark(&self) -> Result<DedupMark> {
        self.storage.dedup_mark()
    }

    /// Artists, albums or tracks that look alike. Only suggestions: nothing is ever merged
    /// unless [`Library::merge`] is called. With `since`, only pairs involving something added
    /// after that mark (what an import brought in).
    pub fn duplicates(
        &self,
        kind: EntityKind,
        since: Option<&DedupMark>,
    ) -> Result<Vec<SimilarPair>> {
        self.storage.duplicates(kind, since)
    }

    /// Fold one artist, album or track into another (the one to keep).
    pub fn merge(&mut self, kind: EntityKind, from: &str, into: &str) -> Result<MergeStats> {
        self.storage.merge_duplicates(kind, from, into)
    }

    /// Record that two artists, albums or tracks are different, so they are not suggested again.
    pub fn separate(&mut self, kind: EntityKind, a: &str, b: &str) -> Result<()> {
        self.storage.separate_duplicates(kind, a, b)
    }

    // --- ratings -----------------------------------------------------------

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

    /// An empty curated playlist with rules for finding tracks to add.
    pub fn create_manual_playlist_with_filter(
        &mut self,
        name: &str,
        description: Option<&str>,
        filter: FilterSpec,
    ) -> Result<Playlist> {
        self.storage
            .create_manual_playlist_with_filter(name, description, &filter)
    }

    /// Tracks offered by a curated playlist's addition rules; never its saved membership.
    pub fn playlist_add_candidates(&self, name: &str, page: Page) -> Result<SearchResult> {
        let playlist = self.storage.playlist(name)?;
        if playlist.kind != PlaylistKind::Manual {
            return Err(Error::InvalidInput(
                "Only manual playlists have curated additions".into(),
            ));
        }
        self.storage.search(&playlist.filter, page)
    }

    pub fn preview_manual_playlist_filter(
        &self,
        name: &str,
        filter: &FilterSpec,
    ) -> Result<(Vec<String>, Vec<String>)> {
        self.storage.preview_manual_playlist_filter(name, filter)
    }

    pub fn update_manual_playlist_filter(
        &mut self,
        name: &str,
        filter: &FilterSpec,
        expected: &[String],
        removed: &[String],
    ) -> Result<Playlist> {
        self.storage
            .update_manual_playlist_filter(name, filter, expected, removed)
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

    /// Persist a draft order for a manual playlist, rejecting stale drafts and membership changes.
    pub fn playlist_reorder(
        &mut self,
        name: &str,
        expected: &[String],
        ordered: &[String],
    ) -> Result<()> {
        self.storage.playlist_reorder(name, expected, ordered)
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

    /// Replace a playlist's whole filter: unlike [`update_playlist`](Self::update_playlist)
    /// this can remove conditions (a tag, an artist). The description and curated membership stay.
    pub fn set_playlist_filter(&mut self, name: &str, filter: FilterSpec) -> Result<Playlist> {
        let mut p = self.storage.playlist(name)?;
        p.filter = self
            .storage
            .update_playlist(&p.id, p.description.as_deref(), &filter)?;
        Ok(p)
    }

    /// Change the specified filters; retain the others. `description`: `None` keeps
    /// the current one, `Some("")` clears it. Curated membership stays unchanged.
    pub fn update_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
        update: FilterSpec,
    ) -> Result<Playlist> {
        let mut p = self.storage.playlist(name)?;
        let merged = p.filter.clone().merged(update).normalized();
        merged.validate()?;
        if let Some(d) = description {
            p.description = Some(d.trim().to_string()).filter(|d| !d.is_empty());
        }
        p.filter = self
            .storage
            .update_playlist(&p.id, p.description.as_deref(), &merged)?;
        Ok(p)
    }

    /// The listening history: every track that was played, how often and when last, most recent
    /// first. A play is recorded each time a track starts, whoever started it (this handle, the
    /// CLI, a remote client). Nothing is known about what was played before the history existed.
    pub fn play_history(&self) -> Result<Vec<PlayStat>> {
        self.storage.play_history()
    }

    /// Set how an album, an artist or a playlist is played: `Some(true)` always shuffled,
    /// `Some(false)` always in order, `None` as the player's shuffle setting says. `id` is a slug
    /// (a playlist may be given by name). Only the preference is stored: whoever starts the
    /// playback applies it (a frontend's Play button, `play_playlist`'s `shuffle`).
    pub fn set_shuffle_pref(
        &mut self,
        kind: EntityKind,
        id: &str,
        shuffle: Option<bool>,
    ) -> Result<()> {
        let id = match kind {
            EntityKind::Playlist => self.storage.playlist(id)?.id,
            EntityKind::Album => self.storage.album(id)?.id,
            EntityKind::Artist => self.storage.artist(id)?.id,
            EntityKind::Track => {
                return Err(Error::InvalidInput(
                    "shuffle is set for an album, an artist or a playlist".into(),
                ));
            }
        };
        self.storage.set_shuffle_pref(kind, &id, shuffle)
    }

    /// Every shuffle preference that is set.
    pub fn shuffle_prefs(&self) -> Result<Vec<ShufflePref>> {
        self.storage.shuffle_prefs()
    }

    /// Rename a playlist. Its ID stays; the new name must not belong to another playlist.
    pub fn rename_playlist(&mut self, name: &str, new_name: &str) -> Result<Playlist> {
        self.storage.rename_playlist(name, new_name)
    }

    /// Playlists whose membership or addition rules draw on this playlist.
    pub fn playlist_dependents(&self, name: &str) -> Result<Vec<Dependent>> {
        self.storage.playlist_dependents(name)
    }

    /// Delete a playlist. Refused while other playlists draw on it unless `detach` is true,
    /// which removes it from their sources (a playlist left with none draws on the whole
    /// library). Returns the playlists it was removed from.
    pub fn delete_playlist(&mut self, name: &str, detach: bool) -> Result<Vec<EntityRef>> {
        let id = self.storage.playlist(name)?.id;
        let dependents = self.storage.delete_playlist(name, detach)?;
        // Its shuffle preference goes with it.
        self.storage
            .set_shuffle_pref(EntityKind::Playlist, &id, None)?;
        Ok(dependents)
    }

    /// Remove source restrictions from playlist membership or addition rules.
    pub fn clear_playlist_sources(&mut self, name: &str) -> Result<Playlist> {
        let mut p = self.storage.playlist(name)?;
        p.filter.clear_sources();
        self.storage
            .update_playlist(&p.id, p.description.as_deref(), &p.filter)?;
        Ok(p)
    }

    // --- this instance -----------------------------------------------------

    /// The name this instance goes by on the network and what it may do.
    pub fn instance(&self) -> Result<Instance> {
        self.storage.instance()
    }

    /// Rename the instance (normalized to a slug).
    pub fn set_instance_name(&mut self, name: &str) -> Result<Instance> {
        self.storage.set_instance_name(name)
    }

    /// Allow this instance to play audio, or make it remote-only.
    pub fn set_playback_allowed(&mut self, allowed: bool) -> Result<Instance> {
        self.storage.set_playback_allowed(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_playlist_order_is_atomic_persistent_and_rejects_stale_drafts() {
        let directory = std::env::temp_dir().join(format!(
            "weave-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("library.db");
        let mut library = Library::open(&database).unwrap();
        let scanned: Vec<_> = ["A", "B", "C", "D"]
            .into_iter()
            .map(|name| ScannedTrack {
                path: format!("/{name}.wav"),
                metadata: weave_player_core::Metadata {
                    title: name.into(),
                    ..Default::default()
                },
            })
            .collect();
        library.import_scanned(&scanned).unwrap();
        let ids: Vec<_> = library
            .search(&Default::default(), Page::ALL)
            .unwrap()
            .tracks
            .into_iter()
            .map(|t| t.id.0)
            .collect();
        let original = ids[..3].to_vec();
        library.create_manual_playlist("Order", None).unwrap();
        library.playlist_add("Order", &original).unwrap();
        let read = |library: &Library| {
            library
                .playlist_tracks("Order", Page::ALL)
                .unwrap()
                .1
                .tracks
                .into_iter()
                .map(|t| t.id.0)
                .collect::<Vec<_>>()
        };
        assert!(
            library
                .playlist_reorder(
                    "Order",
                    &original,
                    &[ids[0].clone(), ids[0].clone(), ids[2].clone()]
                )
                .is_err()
        );
        assert_eq!(read(&library), original);
        let mut draft = original.clone();
        draft.rotate_left(1);
        library
            .playlist_reorder("Order", &original, &draft)
            .unwrap();
        assert_eq!(read(&library), draft);
        assert!(
            library
                .playlist_reorder("Order", &original, &original)
                .is_err()
        );
        assert_eq!(read(&library), draft);
        library.playlist_add("Order", &ids[3..]).unwrap();
        assert!(
            library
                .playlist_reorder("Order", &draft, &original)
                .is_err()
        );
        let mut saved = draft;
        saved.push(ids[3].clone());
        assert_eq!(read(&library), saved);
        library
            .create_playlist("Generated", None, Default::default())
            .unwrap();
        assert!(library.playlist_reorder("Generated", &[], &[]).is_err());
        drop(library);
        let reopened = Library::open(&database).unwrap();
        assert_eq!(read(&reopened), saved);
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    }

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
                    rating_min: Some(4),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p.filter.tag_all, ["bachata"]);
        assert_eq!(lib.playlist("P").unwrap().filter.rating_min, Some(4));
    }

    #[test]
    fn set_playlist_filter_replaces_so_conditions_can_be_removed() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_playlist(
            "P",
            Some("about"),
            FilterSpec {
                tag_all: vec!["bachata".into()],
                artist: Some("aventura".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // Merging cannot drop the artist; replacing can.
        let p = lib
            .set_playlist_filter(
                "P",
                FilterSpec {
                    tag_all: vec!["bachata".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p.filter.tag_all, ["bachata"]);
        let stored = lib.playlist("P").unwrap();
        assert_eq!(stored.filter.artist, None);
        assert_eq!(stored.description.as_deref(), Some("about"));
        // Manual rules can also be replaced without populating membership.
        lib.create_manual_playlist("M", None).unwrap();
        assert!(lib.set_playlist_filter("M", FilterSpec::default()).is_ok());
    }

    #[test]
    fn update_rejects_invalid_merge() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_playlist(
            "P",
            None,
            FilterSpec {
                rating_max: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        let r = lib.update_playlist(
            "P",
            None,
            FilterSpec {
                rating_min: Some(4),
                ..Default::default()
            },
        );
        assert!(matches!(r, Err(Error::InvalidFilter(_))));
        assert_eq!(lib.playlist("P").unwrap().filter.rating_min, None);
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
            path: "x".into(),
            metadata: weave_player_core::Metadata {
                title: "T".into(),
                ..Default::default()
            },
        };
        assert_eq!(
            lib.import_scanned(std::slice::from_ref(&t)).unwrap().added,
            1
        );
        assert_eq!(lib.import_scanned(&[t]).unwrap().skipped, 1);
    }

    fn lib_with(titles: &[&str]) -> Library {
        let mut lib = Library::open_in_memory().unwrap();
        let tracks: Vec<_> = titles
            .iter()
            .map(|t| ScannedTrack {
                path: (*t).into(),
                metadata: weave_player_core::Metadata {
                    title: (*t).into(),
                    artist: Some("x".into()),
                    ..Default::default()
                },
            })
            .collect();
        lib.import_scanned(&tracks).unwrap();
        lib
    }

    #[test]
    fn manual_playlists_take_a_description_and_addition_rules() {
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
                rating_min: Some(4),
                ..Default::default()
            },
        );
        assert_eq!(r.unwrap().filter.rating_min, Some(4));
        assert!(
            lib.playlist_tracks("mix", Page::ALL)
                .unwrap()
                .1
                .tracks
                .is_empty()
        );
    }

    #[test]
    fn curated_addition_rules_never_populate_or_hide_membership_and_reviewed_pruning_is_atomic() {
        let mut lib = lib_with(&["a", "b", "c"]);
        let rules = FilterSpec {
            query: Some("a".into()),
            ..Default::default()
        };
        let p = lib
            .create_manual_playlist_with_filter("Curated", None, rules.clone())
            .unwrap();
        assert_eq!(p.kind, PlaylistKind::Manual);
        assert_eq!(
            lib.playlist_add_candidates("Curated", Page::ALL)
                .unwrap()
                .tracks
                .len(),
            1
        );
        assert!(
            lib.playlist_tracks("Curated", Page::ALL)
                .unwrap()
                .1
                .tracks
                .is_empty()
        );
        // Curation is explicit, and saved addition rules never filter existing membership.
        let expected = vec!["x-c".to_string(), "x-a".to_string(), "x-b".to_string()];
        lib.playlist_add("Curated", &expected).unwrap();
        let read = |lib: &Library| {
            lib.playlist_tracks("Curated", Page::ALL)
                .unwrap()
                .1
                .tracks
                .into_iter()
                .map(|t| t.id.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(read(&lib), expected);
        let (_, removed) = lib
            .preview_manual_playlist_filter("Curated", &rules)
            .unwrap();
        assert_eq!(removed, ["x-c", "x-b"]);
        assert_eq!(read(&lib), expected, "preview never removes tracks");
        assert!(
            lib.update_manual_playlist_filter("Curated", &rules, &expected, &["x-c".into()])
                .is_err()
        );
        assert_eq!(
            read(&lib),
            expected,
            "a stale removal count cannot partially save"
        );
        lib.set_playlist_filter("Curated", FilterSpec::default())
            .unwrap();
        let proposed = FilterSpec {
            query: Some("b".into()),
            ..Default::default()
        };
        let (members, removed) = lib
            .preview_manual_playlist_filter("Curated", &proposed)
            .unwrap();
        lib.update_manual_playlist_filter("Curated", &proposed, &members, &removed)
            .unwrap();
        assert_eq!(read(&lib), ["x-b"]);
        assert_eq!(lib.playlist("Curated").unwrap().filter, proposed);
        assert_eq!(
            lib.search(&FilterSpec::default(), Page::ALL).unwrap().total,
            3
        );
        // A manual source contributes curated membership, never its addition rules.
        lib.set_playlist_filter("Curated", rules).unwrap();
        assert_eq!(
            lib.search(
                &FilterSpec {
                    sources: vec!["Curated".into()],
                    ..Default::default()
                },
                Page::ALL
            )
            .unwrap()
            .tracks[0]
                .id
                .0,
            "x-b"
        );
    }

    #[test]
    fn playlist_tracks_follow_the_kind() {
        let mut lib = lib_with(&["a", "b", "c"]);
        lib.create_manual_playlist("Manual", None).unwrap();
        lib.playlist_add("manual", &["x-c".into(), "x-a".into()])
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
        lib.import_scanned(&[ScannedTrack {
            path: "d".into(),
            metadata: weave_player_core::Metadata {
                title: "d".into(),
                artist: Some("x".into()),
                ..Default::default()
            },
        }])
        .unwrap();
        assert_eq!(titles(&lib, "dynamic").len(), 4);
        assert_eq!(titles(&lib, "manual").len(), 2);
    }

    #[test]
    fn a_manual_playlists_play_order_can_be_changed_and_playing_follows_it() {
        let mut lib = lib_with(&["a", "b", "c"]);
        lib.create_manual_playlist("Mix", None).unwrap();
        lib.playlist_add("mix", &["x-a".into(), "x-b".into(), "x-c".into()])
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
    fn a_track_in_detail_shows_its_links_playlists_and_queue_positions() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.import_scanned(&[ScannedTrack {
            path: "/music/a.mp3".into(),
            metadata: weave_player_core::Metadata {
                title: "a".into(),
                artist: Some("Aventura".into()),
                album: Some("Gold".into()),
                ..Default::default()
            },
        }])
        .unwrap();
        lib.create_manual_playlist("Road", None).unwrap();
        lib.playlist_add("road", &["aventura-gold-a".into()])
            .unwrap();
        lib.queue_add(
            &["aventura-gold-a".into(), "aventura-gold-a".into()],
            None,
            false,
        )
        .unwrap();
        let d = lib.track_detail("aventura-gold-a").unwrap();
        assert_eq!(d.paths, ["/music/a.mp3"]);
        assert_eq!(d.artist.unwrap().id, "aventura");
        assert_eq!(d.album.unwrap().id, "aventura-gold");
        assert_eq!(
            d.playlists
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["road"]
        );
        assert_eq!(d.queue_positions, [1, 2]);
        assert!(d.added_at > 0);
        assert!(matches!(lib.track_detail("nope"), Err(Error::NotFound(_))));
    }
}
