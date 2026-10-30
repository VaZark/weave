//! Bundled `local` backend: read-only file scanning. Files are never modified.

use std::path::Path;

use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::tag::ItemKey;
use walkdir::WalkDir;
use weave_core::{Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, Error, Result, ScannedTrack};

pub const BACKEND_ID: &str = "local";

const EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wv", "ape"];

#[derive(Debug, Default)]
pub struct LocalBackend;

impl Backend for LocalBackend {
    fn id(&self) -> &str {
        BACKEND_ID
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        let cap = |group, name: &str, state| CapabilityDescriptor { group, name: name.into(), state };
        let off = CapabilityState { supported: false, authorized: true, available: false };
        vec![
            cap(CapabilityGroup::Library, "read", CapabilityState::READY),
            cap(CapabilityGroup::Metadata, "read", CapabilityState::READY),
            // Planned for 0.a2.
            cap(CapabilityGroup::Playback, "control", off),
        ]
    }
}

impl LocalBackend {
    /// Recursively scan `root` (or a single file) for audio files. Unreadable
    /// or untagged files are still indexed, with the file stem as title.
    pub fn scan(&self, root: &Path) -> Result<Vec<ScannedTrack>> {
        if !root.exists() {
            return Err(Error::NotFound(format!("path {}", root.display())));
        }
        let mut out = Vec::new();
        for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
            let entry = entry.map_err(|e| Error::Backend(e.to_string()))?;
            if entry.file_type().is_file() && is_audio(entry.path()) {
                out.push(read_track(entry.path()));
            }
        }
        Ok(out)
    }
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn read_track(path: &Path) -> ScannedTrack {
    let external_id = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut track = ScannedTrack { external_id, title: stem, artist: None, album: None, bpm: None };

    let Ok(file) = lofty::read_from_path(path) else { return track };
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return track };
    if let Some(t) = tag.title().filter(|t| !t.trim().is_empty()) {
        track.title = t.into_owned();
    }
    track.artist = tag.artist().map(|s| s.into_owned()).filter(|s| !s.trim().is_empty());
    track.album = tag.album().map(|s| s.into_owned()).filter(|s| !s.trim().is_empty());
    track.bpm = [ItemKey::Bpm, ItemKey::IntegerBpm]
        .iter()
        .filter_map(|k| tag.get_string(k))
        .find_map(|s| s.trim().parse::<f64>().ok())
        .filter(|b| b.is_finite() && *b > 0.0);
    track
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_audio_by_extension_without_touching_files() {
        let dir = std::env::temp_dir().join(format!("weave-local-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/Song One.MP3"), b"not really audio").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        let tracks = LocalBackend.scan(&dir).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Song One");
        assert_eq!(std::fs::read(dir.join("sub/Song One.MP3")).unwrap(), b"not really audio");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_path_is_not_found() {
        assert!(matches!(LocalBackend.scan(Path::new("/nonexistent/weave")), Err(Error::NotFound(_))));
    }
}
