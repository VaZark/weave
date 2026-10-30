//! The one engine API. Frontends call this and never touch storage directly.

use std::path::Path;

use weave_core::{Backend, CapabilityDescriptor, Error, FilterSpec, Result, Track};
use weave_local::LocalBackend;
use weave_storage::Storage;

pub use weave_storage::{ImportStats, SmartPlaylist};

pub struct BackendInfo {
    pub id: String,
    pub capabilities: Vec<CapabilityDescriptor>,
}

pub struct Library {
    storage: Storage,
    local: LocalBackend,
}

impl Library {
    pub fn open(db_path: &Path) -> Result<Self> {
        Ok(Self { storage: Storage::open(db_path)?, local: LocalBackend })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self { storage: Storage::open_in_memory()?, local: LocalBackend })
    }

    // --- library -----------------------------------------------------------

    pub fn import(&mut self, path: &Path) -> Result<ImportStats> {
        let scanned = self.local.scan(path)?;
        self.storage.import_tracks(self.local.id(), &scanned)
    }

    pub fn search(&self, filter: &FilterSpec) -> Result<Vec<Track>> {
        self.storage.search(filter)
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

    // --- smart playlists ---------------------------------------------------

    pub fn create_playlist(&mut self, name: &str, filter: FilterSpec) -> Result<SmartPlaylist> {
        self.storage.create_playlist(name, &filter)
    }

    pub fn playlists(&self) -> Result<Vec<SmartPlaylist>> {
        self.storage.playlists()
    }

    pub fn playlist(&self, name: &str) -> Result<SmartPlaylist> {
        self.storage.playlist(name)
    }

    /// Resolve membership against current library state.
    pub fn playlist_tracks(&self, name: &str) -> Result<(SmartPlaylist, Vec<Track>)> {
        let p = self.storage.playlist(name)?;
        let tracks = self.storage.search(&p.filter)?;
        Ok((p, tracks))
    }

    /// Change the specified filters; retain the others.
    pub fn update_playlist(&mut self, name: &str, update: FilterSpec) -> Result<SmartPlaylist> {
        let mut p = self.storage.playlist(name)?;
        let merged = p.filter.clone().merged(update).normalized();
        merged.validate()?;
        self.storage.update_playlist_filter(&p.id, &merged)?;
        p.filter = merged;
        Ok(p)
    }

    // --- backends ----------------------------------------------------------

    pub fn backends(&self) -> Vec<BackendInfo> {
        vec![BackendInfo { id: self.local.id().into(), capabilities: self.local.capabilities() }]
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
        let base = FilterSpec { tag_all: vec!["bachata".into()], ..Default::default() };
        lib.create_playlist("P", base).unwrap();
        let p = lib
            .update_playlist("p", FilterSpec { bpm_min: Some(90.0), ..Default::default() })
            .unwrap();
        assert_eq!(p.filter.tag_all, ["bachata"]);
        assert_eq!(lib.playlist("P").unwrap().filter.bpm_min, Some(90.0));
    }

    #[test]
    fn update_rejects_invalid_merge() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_playlist("P", FilterSpec { bpm_max: Some(100.0), ..Default::default() }).unwrap();
        let r = lib.update_playlist("P", FilterSpec { bpm_min: Some(120.0), ..Default::default() });
        assert!(matches!(r, Err(Error::InvalidFilter(_))));
        assert_eq!(lib.playlist("P").unwrap().filter.bpm_min, None);
    }

    #[test]
    fn unknown_backend() {
        let lib = Library::open_in_memory().unwrap();
        assert!(lib.backend("local").is_ok());
        assert!(matches!(lib.backend("spotify"), Err(Error::NotFound(_))));
    }
}
