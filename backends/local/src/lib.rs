//! Bundled `local` backend: read-only file scanning, lyrics lookup and (with the `audio`
//! feature) playback. Files are never modified.

#[cfg(feature = "audio")]
mod playback;

use std::path::{Path, PathBuf};

use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use walkdir::WalkDir;
use weave_core::{
    Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, Error, Lyrics, Metadata,
    Result, ScannedTrack,
};

#[cfg(feature = "audio")]
pub use playback::LocalPlayback;

pub const BACKEND_ID: &str = "local";

#[derive(Debug, Default)]
pub struct LocalBackend;

impl Backend for LocalBackend {
    fn id(&self) -> &str {
        BACKEND_ID
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        let cap = |group, name: &str, state| CapabilityDescriptor {
            group,
            name: name.into(),
            state,
        };
        // Whether a sound device is actually present is only known when a host opens one.
        let audio = CapabilityState {
            supported: cfg!(feature = "audio"),
            authorized: true,
            available: cfg!(feature = "audio"),
        };
        vec![
            cap(CapabilityGroup::Library, "read", CapabilityState::READY),
            cap(CapabilityGroup::Metadata, "read", CapabilityState::READY),
            cap(CapabilityGroup::Playback, "control", audio),
            cap(CapabilityGroup::Assets, "lyrics", CapabilityState::READY),
        ]
    }
}

impl LocalBackend {
    /// Lyrics for a source: a `.lrc` file next to the audio file, else lyrics embedded in
    /// its tags (synchronized when they contain LRC timestamps). `None` when there are none.
    pub fn lyrics(&self, external_id: &str) -> Result<Option<Lyrics>> {
        let path = PathBuf::from(external_id);
        for ext in ["lrc", "LRC"] {
            if let Ok(bytes) = std::fs::read(path.with_extension(ext))
                && let Some(l) = Lyrics::parse(&String::from_utf8_lossy(&bytes))
            {
                return Ok(Some(l));
            }
        }
        let Ok(file) = lofty::read_from_path(&path) else {
            return Ok(None);
        };
        Ok(file
            .primary_tag()
            .or_else(|| file.first_tag())
            .and_then(|tag| tag.get_string(&ItemKey::Lyrics).and_then(Lyrics::parse)))
    }

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
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
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
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut m = Metadata {
        title: stem,
        ..Default::default()
    };

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
    ScannedTrack {
        external_id,
        metadata: m,
    }
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
        let dir =
            std::env::temp_dir().join(format!("weave-local-{}", uuid::Uuid::new_v4().simple()));
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

        let mut titles: Vec<_> = LocalBackend
            .scan(&dir)
            .unwrap()
            .into_iter()
            .map(|t| t.metadata.title)
            .collect();
        titles.sort();
        assert_eq!(titles, ["Song One", "mislabeled", "no_extension"]);
        assert_eq!(std::fs::read(dir.join("sub/Song One.wav")).unwrap(), wav);
        assert_eq!(
            std::fs::read(dir.join("fake.mp3")).unwrap(),
            b"not really audio"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn write_silent_wav(path: &Path, secs: u32) {
        let mut w = tiny_wav();
        let data_len = 8000 * 2 * secs;
        w.truncate(w.len() - 4);
        w.extend(data_len.to_le_bytes());
        w.extend(std::iter::repeat_n(0u8, data_len as usize));
        let riff = (w.len() - 8) as u32;
        w[4..8].copy_from_slice(&riff.to_le_bytes());
        std::fs::write(path, w).unwrap();
    }

    #[test]
    fn lyrics_come_from_a_sidecar_lrc_file() {
        let dir = temp_dir();
        let song = dir.join("Song.wav");
        write_silent_wav(&song, 1);
        std::fs::write(dir.join("Song.lrc"), "[00:01.00]hello\n[00:02.00]world").unwrap();
        let l = LocalBackend
            .lyrics(&song.to_string_lossy())
            .unwrap()
            .unwrap();
        assert!(l.synced);
        assert_eq!(l.lines.len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lyrics_fall_back_to_the_embedded_tag() {
        use lofty::config::WriteOptions;
        use lofty::tag::{Tag, TagType};
        let dir = temp_dir();
        let song = dir.join("Tagged.wav");
        write_silent_wav(&song, 1);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert_text(ItemKey::Lyrics, "[00:05.00]embedded line".into());
        tag.save_to_path(&song, WriteOptions::default()).unwrap();

        let l = LocalBackend
            .lyrics(&song.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(
            (l.synced, l.lines[0].text.as_str()),
            (true, "embedded line")
        );

        // a sidecar wins over the embedded text
        std::fs::write(dir.join("Tagged.lrc"), "[00:01.00]sidecar line").unwrap();
        let l = LocalBackend
            .lyrics(&song.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(l.lines[0].text, "sidecar line");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn no_lyrics_is_none_not_an_error() {
        let dir = temp_dir();
        let song = dir.join("Plain.wav");
        write_silent_wav(&song, 1);
        assert_eq!(LocalBackend.lyrics(&song.to_string_lossy()).unwrap(), None);
        assert_eq!(LocalBackend.lyrics("/nonexistent/song.mp3").unwrap(), None);
        std::fs::write(dir.join("Plain.lrc"), "  \n").unwrap();
        assert_eq!(
            LocalBackend.lyrics(&song.to_string_lossy()).unwrap(),
            None,
            "an empty sidecar is ignored"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn playback_capability_follows_the_audio_feature() {
        let cap = LocalBackend
            .capabilities()
            .into_iter()
            .find(|c| c.group == CapabilityGroup::Playback)
            .unwrap();
        assert_eq!(cap.state.supported, cfg!(feature = "audio"));
    }

    #[cfg(feature = "audio")]
    mod audio {
        use std::time::{Duration, Instant};

        use weave_core::{Playback, SourceRef};

        use super::*;

        fn source(path: &Path) -> SourceRef {
            SourceRef {
                backend_id: BACKEND_ID.into(),
                external_id: path.to_string_lossy().into_owned(),
            }
        }

        /// Plays silence, but still needs an output device; skipped where there is none.
        #[test]
        fn plays_pauses_seeks_and_finishes() {
            let Ok(mut p) = LocalPlayback::open() else {
                eprintln!("no audio output device; skipping");
                return;
            };
            let dir = temp_dir();
            let song = dir.join("silence.wav");
            write_silent_wav(&song, 2);

            p.start(&source(&song), Duration::ZERO).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            assert!(p.position() > Duration::from_millis(100) && !p.finished());

            p.pause();
            let frozen = p.position();
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                p.position().abs_diff(frozen) < Duration::from_millis(50),
                "paused position holds"
            );
            p.resume();

            p.seek(Duration::from_millis(1800)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !p.finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                p.finished(),
                "the track should finish soon after seeking near its end"
            );

            p.start(&source(&song), Duration::from_millis(500)).unwrap();
            assert!(
                p.position() >= Duration::from_millis(400),
                "start can begin partway in"
            );
            p.stop();
            assert!(!p.finished() && p.position() == Duration::ZERO);
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn rejects_foreign_sources_and_missing_files() {
            let Ok(mut p) = LocalPlayback::open() else {
                return;
            };
            let foreign = SourceRef {
                backend_id: "spotify".into(),
                external_id: "x".into(),
            };
            assert!(matches!(
                p.start(&foreign, Duration::ZERO),
                Err(Error::Backend(_))
            ));
            let missing = source(Path::new("/nonexistent/song.mp3"));
            assert!(matches!(
                p.start(&missing, Duration::ZERO),
                Err(Error::Backend(_))
            ));
            assert!(p.seek(Duration::from_secs(1)).is_err(), "nothing is loaded");
        }
    }

    #[test]
    fn missing_path_is_not_found() {
        assert!(matches!(
            LocalBackend.scan(Path::new("/nonexistent/weave")),
            Err(Error::NotFound(_))
        ));
    }
}
