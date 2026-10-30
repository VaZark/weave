//! Audio output for the `local` backend: rodio (symphonia decoders, cpal output).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use weave_core::{Error, Playback, Result, SourceRef};

use crate::BACKEND_ID;

fn backend_err(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Backend(format!("{what}: {e}"))
}

/// rodio cannot seek in the first moments of a track (`RandomAccessNotSupported`), so
/// anything earlier than this is done by starting the file over.
const MIN_SEEK: Duration = Duration::from_millis(250);

pub struct LocalPlayback {
    // Dropping the device sink stops all sound, so it lives as long as the player.
    sink: MixerDeviceSink,
    player: Option<Player>,
    source: Option<SourceRef>,
    paused: bool,
    volume: f32,
}

impl LocalPlayback {
    /// Open the default output device.
    pub fn open() -> Result<Self> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| backend_err("cannot open the audio output", e))?;
        sink.log_on_drop(false);
        Ok(Self {
            sink,
            player: None,
            source: None,
            paused: false,
            volume: 1.0,
        })
    }
}

impl Playback for LocalPlayback {
    fn start(&mut self, source: &SourceRef, from: Duration) -> Result<()> {
        if source.backend_id != BACKEND_ID {
            return Err(Error::Backend(format!(
                "the local backend cannot play a {} source",
                source.backend_id
            )));
        }
        self.stop();
        let path = Path::new(&source.external_id);
        let file = File::open(path).map_err(|e| backend_err(&path.display().to_string(), e))?;
        let decoder = Decoder::try_from(BufReader::new(file))
            .map_err(|e| backend_err(&path.display().to_string(), e))?;
        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.volume);
        player.append(decoder);
        // Best effort: if the seek fails the track simply plays from the start.
        if from >= MIN_SEEK {
            let _ = player.try_seek(from);
        }
        self.player = Some(player);
        self.source = Some(source.clone());
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
        self.source = None;
        self.paused = false;
    }

    fn seek(&mut self, to: Duration) -> Result<()> {
        let Some(player) = &self.player else {
            return Err(Error::InvalidInput("nothing is playing".into()));
        };
        if to >= MIN_SEEK {
            return player
                .try_seek(to)
                .map_err(|e| backend_err("cannot seek", e));
        }
        // Too early to seek: start the same file over (staying paused if it was paused).
        let source = self.source.clone().expect("a player always has a source");
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

impl crate::LocalBackend {
    /// A player on the default output device (needs the `audio` feature and a device).
    pub fn playback(&self) -> Result<Box<dyn Playback>> {
        Ok(Box::new(LocalPlayback::open()?))
    }
}
