//! Bundled `local` backend: read-only file scanning. Files are never modified.

use std::path::Path;

use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::prelude::*;
use lofty::tag::ItemKey;
use walkdir::WalkDir;
use weave_core::{
    Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, Error, Metadata, Result, ScannedTrack,
};

pub const BACKEND_ID: &str = "local";

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
    /// Recursively scan `root` (or a single file) for audio files, detected by
    /// content. Untagged or partly unreadable audio is still indexed, with the
    /// file stem as title.
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

/// Audio is recognized by file content (magic bytes), not by extension, so
/// mislabeled files are handled correctly and non-audio files are never indexed.
fn is_audio(path: &Path) -> bool {
    // `Probe::open` would fall back to the extension when the content is not
    // recognized; build the probe from a bare reader so only content counts.
    let Ok(file) = std::fs::File::open(path) else { return false };
    Probe::new(std::io::BufReader::new(file))
        .guess_file_type()
        .is_ok_and(|p| p.file_type().is_some())
}

fn read_track(path: &Path) -> ScannedTrack {
    let external_id = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut m = Metadata { title: stem, ..Default::default() };

    if let Ok(file) = lofty::read_from_path(path) {
        let ms = file.properties().duration().as_millis() as u64;
        m.duration_ms = (ms > 0).then_some(ms);
        if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
            let text = |v: Option<std::borrow::Cow<str>>| {
                v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
            };
            let item = |k: ItemKey| text(tag.get_string(&k).map(Into::into));
            if let Some(t) = text(tag.title()) {
                m.title = t;
            }
            m.artist = text(tag.artist());
            m.album = text(tag.album());
            m.genre = text(tag.genre());
            m.album_artist = item(ItemKey::AlbumArtist);
            m.composer = item(ItemKey::Composer);
            m.year = tag.year().and_then(|y| i32::try_from(y).ok());
            m.track_number = tag.track();
            m.disc_number = tag.disk();
            m.bpm = [ItemKey::Bpm, ItemKey::IntegerBpm]
                .into_iter()
                .filter_map(item)
                .find_map(|s| s.parse::<f64>().ok())
                .filter(|b| b.is_finite() && *b > 0.0);
        }
    }
    ScannedTrack { external_id, metadata: m }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smallest well-formed WAV: PCM header and an empty data chunk.
    fn tiny_wav() -> Vec<u8> {
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend(36u32.to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        for v in [1u16, 1] {
            w.extend(v.to_le_bytes()); // PCM, mono
        }
        w.extend(8000u32.to_le_bytes());
        w.extend(16000u32.to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(0u32.to_le_bytes());
        w
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("weave-local-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        dir
    }

    #[test]
    fn detects_audio_by_content_without_touching_files() {
        let dir = temp_dir();
        let wav = tiny_wav();
        std::fs::write(dir.join("sub/Song One.wav"), &wav).unwrap();
        std::fs::write(dir.join("mislabeled.dat"), &wav).unwrap(); // audio, wrong extension
        std::fs::write(dir.join("no_extension"), &wav).unwrap();
        std::fs::write(dir.join("fake.mp3"), b"not really audio").unwrap(); // right extension, not audio
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.join("empty.flac"), b"").unwrap();

        let mut titles: Vec<_> =
            LocalBackend.scan(&dir).unwrap().into_iter().map(|t| t.metadata.title).collect();
        titles.sort();
        assert_eq!(titles, ["Song One", "mislabeled", "no_extension"]);
        assert_eq!(std::fs::read(dir.join("sub/Song One.wav")).unwrap(), wav);
        assert_eq!(std::fs::read(dir.join("fake.mp3")).unwrap(), b"not really audio");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_path_is_not_found() {
        assert!(matches!(LocalBackend.scan(Path::new("/nonexistent/weave")), Err(Error::NotFound(_))));
    }
}
