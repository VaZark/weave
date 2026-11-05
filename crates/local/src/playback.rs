//! Audio output for local files: rodio (symphonia decoders, cpal output).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};
use weave_player_core::{Error, Playback, Result};

fn player_err(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Player(format!("{what}: {e}"))
}

/// rodio cannot seek in the first moments of a track (`RandomAccessNotSupported`), so
/// anything earlier than this is done by starting the file over.
const MIN_SEEK: Duration = Duration::from_millis(250);

/// How many samples the tap collects before it hands them to the spectrum.
const TAP_BATCH: usize = 512;

/// A source that plays what its inner one does and hands the samples to the spectrum
/// (`weave_player_core::spectrum`) on the way to the device.
struct Tap<S> {
    inner: S,
    batch: Vec<f32>,
}

impl<S: Source> Tap<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            batch: Vec::with_capacity(TAP_BATCH),
        }
    }

    fn flush(&mut self) {
        if !self.batch.is_empty() {
            weave_player_core::spectrum::feed(
                self.inner.sample_rate().get(),
                self.inner.channels().get(),
                &self.batch,
            );
            self.batch.clear();
        }
    }
}

impl<S: Source> Iterator for Tap<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next();
        match sample {
            Some(v) => {
                self.batch.push(v);
                if self.batch.len() >= TAP_BATCH {
                    self.flush();
                }
            }
            None => self.flush(),
        }
        sample
    }
}

impl<S: Source> Source for Tap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> std::result::Result<(), SeekError> {
        self.batch.clear();
        self.inner.try_seek(pos)
    }
}

pub struct LocalPlayback {
    // Dropping the device sink stops all sound, so it lives as long as the player.
    sink: MixerDeviceSink,
    player: Option<Player>,
    path: Option<String>,
    paused: bool,
    volume: f32,
}

impl LocalPlayback {
    /// Open the default output device.
    pub fn open() -> Result<Self> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| player_err("cannot open the audio output", e))?;
        sink.log_on_drop(false);
        Ok(Self {
            sink,
            player: None,
            path: None,
            paused: false,
            volume: 1.0,
        })
    }
}

impl Playback for LocalPlayback {
    fn start(&mut self, path: &str, from: Duration) -> Result<()> {
        self.stop();
        let path = Path::new(path);
        let file = File::open(path).map_err(|e| player_err(&path.display().to_string(), e))?;
        let decoder = Decoder::try_from(BufReader::new(file))
            .map_err(|e| player_err(&path.display().to_string(), e))?;
        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.volume);
        player.append(Tap::new(decoder));
        // Best effort: if the seek fails the track simply plays from the start.
        if from >= MIN_SEEK {
            let _ = player.try_seek(from);
        }
        self.player = Some(player);
        self.path = Some(path.to_string_lossy().into_owned());
        self.paused = false;
        Ok(())
    }

    fn pause(&mut self) {
        if let Some(p) = &self.player {
            p.pause();
            self.paused = true;
        }
    }

    fn resume(&mut self) {
        if let Some(p) = &self.player {
            p.play();
            self.paused = false;
        }
    }

    fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
        self.path = None;
        self.paused = false;
    }

    fn seek(&mut self, to: Duration) -> Result<()> {
        let Some(player) = &self.player else {
            return Err(Error::InvalidInput("nothing is playing".into()));
        };
        if to >= MIN_SEEK {
            return player
                .try_seek(to)
                .map_err(|e| player_err("cannot seek", e));
        }
        // Too early to seek: start the same file over (staying paused if it was paused).
        let source = self.path.clone().expect("a player always has a file");
        let was_paused = self.paused;
        self.start(&source, Duration::ZERO)?;
        if was_paused {
            self.pause();
        }
        Ok(())
    }

    fn position(&self) -> Duration {
        self.player.as_ref().map_or(Duration::ZERO, Player::get_pos)
    }

    fn finished(&self) -> bool {
        self.player.as_ref().is_some_and(Player::empty)
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(p) = &self.player {
            p.set_volume(self.volume);
        }
    }
}

impl crate::LocalFiles {
    /// A player on the default output device (needs the `audio` feature and a device).
    pub fn playback(&self) -> Result<Box<dyn Playback>> {
        Ok(Box::new(LocalPlayback::open()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::source::SineWave;

    #[test]
    fn the_tap_passes_samples_on_unchanged_and_feeds_the_spectrum() {
        let sine: Vec<f32> = SineWave::new(1_000.0).take(8192).collect();
        let tapped: Vec<f32> = Tap::new(SineWave::new(1_000.0)).take(8192).collect();
        assert_eq!(sine, tapped, "the audio is not changed");
        let mut b = weave_player_core::spectrum::bands();
        for _ in 0..8 {
            b = weave_player_core::spectrum::bands();
        }
        assert!(
            b.iter().any(|l| *l > 0.3),
            "a tone was heard by the spectrum: {b:?}"
        );
    }
}
