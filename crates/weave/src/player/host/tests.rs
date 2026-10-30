use weave_core::{PlayStatus, PlayerState};

use super::*;

// --- the follower: pure ---------------------------------------------------------------------

fn state(status: PlayStatus, start_seq: u64, seek_seq: u64) -> PlayerState {
    PlayerState {
        status,
        current: Some(0),
        start_seq,
        seek_seq,
        ..Default::default()
    }
}

#[test]
fn follower_starts_a_new_track_once_and_applies_volume_first() {
    let mut f = Follower::default();
    let st = state(PlayStatus::Playing, 1, 0);
    assert_eq!(f.step(&st, 0), [Action::Volume(1.0), Action::Start(0)]);
    assert!(f.step(&st, 500).is_empty(), "nothing changed");
}

#[test]
fn follower_restarts_when_the_start_counter_changes_even_for_the_same_track() {
    let mut f = Follower::default();
    f.step(&state(PlayStatus::Playing, 1, 0), 0);
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 2, 0), 0),
        [Action::Start(0)]
    );
}

#[test]
fn follower_joins_a_session_that_is_already_playing_at_its_position() {
    let mut f = Follower::default();
    let actions = f.step(&state(PlayStatus::Playing, 5, 0), 40_000);
    assert_eq!(
        actions[1],
        Action::Start(40_000),
        "a host started late resumes in place"
    );
}

#[test]
fn follower_plays_a_track_it_noticed_a_tick_late_from_its_beginning() {
    let mut f = Follower::default();
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 1, 0), 100)[1],
        Action::Start(0)
    );
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 2, 0), 999)[0],
        Action::Start(0)
    );
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 3, 0), 1_000)[0],
        Action::Start(1_000),
        "from a second in, a late player resumes in place"
    );
}

#[test]
fn follower_pause_resume_seek_stop() {
    let mut f = Follower::default();
    f.step(&state(PlayStatus::Playing, 1, 0), 0);
    assert_eq!(f.step(&state(PlayStatus::Paused, 1, 0), 0), [Action::Pause]);
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 1, 0), 0),
        [Action::Resume]
    );
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 1, 1), 9_000),
        [Action::Seek(9_000)]
    );
    assert_eq!(
        f.step(&state(PlayStatus::Stopped, 1, 1), 9_000),
        [Action::Stop]
    );
    assert!(
        f.step(&state(PlayStatus::Stopped, 1, 1), 9_000).is_empty(),
        "stopping twice does nothing"
    );
}

#[test]
fn follower_loads_a_paused_session_and_pauses_it() {
    let mut f = Follower::default();
    let actions = f.step(&state(PlayStatus::Paused, 3, 0), 12_000);
    assert_eq!(actions[1..], [Action::Start(12_000), Action::Pause]);
}

#[test]
fn follower_stopped_session_with_nothing_loaded_does_nothing() {
    let mut f = Follower::default();
    let actions = f.step(&PlayerState::default(), 0);
    assert_eq!(actions, [Action::Volume(1.0)]);
}

#[test]
fn follower_ignores_a_seek_that_arrives_together_with_a_new_track() {
    let mut f = Follower::default();
    f.step(&state(PlayStatus::Playing, 1, 0), 0);
    assert_eq!(
        f.step(&state(PlayStatus::Playing, 2, 1), 0),
        [Action::Start(0)]
    );
}

// --- desktop controller requests ---------------------------------------------------------------

mod remote {
    use std::cell::Cell;
    use std::rc::Rc;

    use weave_core::{Metadata, RepeatMode, ScannedTrack};

    use super::*;
    use crate::player::media::Remote;

    fn lib() -> (Library, Rc<Cell<i64>>) {
        let clock = Rc::new(Cell::new(1_000_000));
        let c = clock.clone();
        let mut lib = Library::open_in_memory()
            .unwrap()
            .with_determinism(1, move || c.get());
        let tracks: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|t| ScannedTrack {
                external_id: (*t).into(),
                metadata: Metadata {
                    title: (*t).into(),
                    artist: Some("x".into()),
                    duration_ms: Some(180_000),
                    ..Default::default()
                },
            })
            .collect();
        lib.import_scanned("fake", &tracks).unwrap();
        lib.queue_add(
            &["x-a-fake".into(), "x-b-fake".into(), "x-c-fake".into()],
            None,
            false,
        )
        .unwrap();
        (lib, clock)
    }

    fn title(lib: &mut Library) -> String {
        lib.session()
            .unwrap()
            .current()
            .map(|t| t.metadata.title.clone())
            .unwrap_or_default()
    }

    #[test]
    fn play_pause_toggles_and_starts_an_idle_queue() {
        let (mut lib, _) = lib();
        assert!(!apply_remote(&mut lib, &Remote::PlayPause).unwrap());
        let st = lib.player_state().unwrap();
        assert_eq!(st.status, PlayStatus::Playing);
        assert_eq!(
            title(&mut lib),
            "a",
            "an idle queue starts at its first track"
        );
        apply_remote(&mut lib, &Remote::PlayPause).unwrap();
        assert_eq!(lib.player_state().unwrap().status, PlayStatus::Paused);
        apply_remote(&mut lib, &Remote::PlayPause).unwrap();
        assert_eq!(lib.player_state().unwrap().status, PlayStatus::Playing);
    }

    #[test]
    fn play_pause_stop_next_previous() {
        let (mut lib, _) = lib();
        apply_remote(&mut lib, &Remote::Play).unwrap();
        apply_remote(&mut lib, &Remote::Play).unwrap(); // already playing: nothing happens
        apply_remote(&mut lib, &Remote::Next).unwrap();
        assert_eq!(title(&mut lib), "b");
        apply_remote(&mut lib, &Remote::Previous).unwrap();
        assert_eq!(title(&mut lib), "a");
        apply_remote(&mut lib, &Remote::Pause).unwrap();
        apply_remote(&mut lib, &Remote::Pause).unwrap(); // already paused: not an error
        assert_eq!(lib.player_state().unwrap().status, PlayStatus::Paused);
        apply_remote(&mut lib, &Remote::Play).unwrap();
        assert_eq!(lib.player_state().unwrap().status, PlayStatus::Playing);
        apply_remote(&mut lib, &Remote::Stop).unwrap();
        assert_eq!(lib.player_state().unwrap().status, PlayStatus::Stopped);
        apply_remote(&mut lib, &Remote::Play).unwrap();
        assert_eq!(
            lib.player_state().unwrap().status,
            PlayStatus::Playing,
            "play after stop restarts the track"
        );
    }

    #[test]
    fn seeking_relative_and_absolute_never_goes_before_the_start() {
        let (mut lib, clock) = lib();
        apply_remote(&mut lib, &Remote::Play).unwrap();
        clock.set(clock.get() + 20_000);
        apply_remote(&mut lib, &Remote::SeekBy(15_000)).unwrap();
        assert_eq!(lib.session().unwrap().position, Duration::from_secs(35));
        apply_remote(&mut lib, &Remote::SeekBy(-60_000)).unwrap();
        assert_eq!(lib.session().unwrap().position, Duration::ZERO);
        apply_remote(&mut lib, &Remote::SetPosition(90_000)).unwrap();
        assert_eq!(lib.session().unwrap().position, Duration::from_secs(90));
    }

    #[test]
    fn volume_shuffle_and_repeat_pass_through() {
        let (mut lib, _) = lib();
        apply_remote(&mut lib, &Remote::SetVolume(0.5)).unwrap();
        apply_remote(&mut lib, &Remote::SetShuffle(true)).unwrap();
        apply_remote(&mut lib, &Remote::SetRepeat(RepeatMode::Playlist)).unwrap();
        let st = lib.player_state().unwrap();
        assert_eq!(
            (st.volume, st.shuffle, st.repeat),
            (0.5, true, RepeatMode::Playlist)
        );
    }

    #[test]
    fn quit_asks_the_host_to_exit_and_bad_requests_are_errors_not_panics() {
        let (mut lib, _) = lib();
        assert!(apply_remote(&mut lib, &Remote::Quit).unwrap());
        assert!(apply_remote(&mut lib, &Remote::SetVolume(7.0)).is_err());
        let mut empty = Library::open_in_memory().unwrap();
        assert!(
            apply_remote(&mut empty, &Remote::SeekBy(1_000)).is_err(),
            "nothing queued"
        );
        assert!(!apply_remote(&mut empty, &Remote::Next).unwrap());
    }

    #[test]
    fn the_snapshot_describes_the_session_for_the_desktop() {
        let (mut lib, _) = lib();
        lib.play_track("x-b-fake").unwrap();
        lib.set_volume(0.25).unwrap();
        let snap = snapshot_of(&lib.session().unwrap(), Some("la la".into()));
        let track = snap.track.unwrap();
        assert_eq!(
            (
                track.title.as_str(),
                track.artist.as_deref(),
                track.length_ms,
                track.lyrics.as_deref()
            ),
            ("b", Some("x"), Some(180_000), Some("la la"))
        );
        assert_eq!(
            (snap.status, snap.volume, track.key),
            (PlayStatus::Playing, 0.25, 1)
        );
        assert!(
            snapshot_of(&Library::open_in_memory().unwrap().session().unwrap(), None)
                .track
                .is_none()
        );
    }
}

// --- the host against a fake player ------------------------------------------------------------
// Needs sources the engine considers playable, which only exist when audio support is built.

#[cfg(feature = "audio")]
mod fake_player {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use weave_core::{RepeatMode, ScannedTrack, SleepSetting, SourceRef};

    use super::*;

    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        finished: bool,
        position: Duration,
        fail_start: bool,
    }

    struct FakePlayback(Rc<RefCell<Fake>>);

    impl Playback for FakePlayback {
        fn start(&mut self, source: &SourceRef, from: Duration) -> weave_core::Result<()> {
            let mut f = self.0.borrow_mut();
            if f.fail_start {
                return Err(Error::Backend("device went away".into()));
            }
            let file = Path::new(&source.external_id)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            f.calls.push(format!("start {file} @{}", from.as_millis()));
            f.position = from;
            f.finished = false;
            Ok(())
        }
        fn pause(&mut self) {
            self.0.borrow_mut().calls.push("pause".into());
        }
        fn resume(&mut self) {
            self.0.borrow_mut().calls.push("resume".into());
        }
        fn stop(&mut self) {
            self.0.borrow_mut().calls.push("stop".into());
        }
        fn seek(&mut self, to: Duration) -> weave_core::Result<()> {
            self.0
                .borrow_mut()
                .calls
                .push(format!("seek {}", to.as_millis()));
            Ok(())
        }
        fn position(&self) -> Duration {
            self.0.borrow().position
        }
        fn finished(&self) -> bool {
            self.0.borrow().finished
        }
        fn set_volume(&mut self, volume: f32) {
            self.0.borrow_mut().calls.push(format!("volume {volume}"));
        }
    }

    fn wav() -> Vec<u8> {
        let data = 16_000u32;
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend((36 + data).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(8000u32.to_le_bytes());
        w.extend(16_000u32.to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(data.to_le_bytes());
        w.extend(std::iter::repeat_n(0u8, data as usize));
        w
    }

    struct Rig {
        lib: Library,
        host: Host,
        fake: Rc<RefCell<Fake>>,
        clock: Rc<Cell<i64>>,
        dir: PathBuf,
    }

    impl Rig {
        /// `a`, `b`, `c` are playable local files.
        fn new() -> Rig {
            let dir = std::env::temp_dir().join(format!(
                "weave-host-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            for name in ["a", "b", "c"] {
                std::fs::write(dir.join(format!("{name}.wav")), wav()).unwrap();
            }
            let clock = Rc::new(Cell::new(1_000_000));
            let c = clock.clone();
            let mut lib = Library::open_in_memory()
                .unwrap()
                .with_determinism(1, move || c.get());
            assert_eq!(lib.import(&dir).unwrap().added, 3);
            let fake = Rc::new(RefCell::new(Fake::default()));
            let mut host = Host::new(Some(Box::new(FakePlayback(fake.clone()))));
            host.report_every = Duration::ZERO;
            Rig {
                lib,
                host,
                fake,
                clock,
                dir,
            }
        }

        fn id(&self, title: &str) -> String {
            format!("unknown-{title}-local")
        }

        fn tick(&mut self) -> Vec<PlayerEvent> {
            self.host
                .tick(&mut self.lib)
                .unwrap_or_else(|_| panic!("tick failed"))
        }

        fn calls(&self) -> Vec<String> {
            self.fake.borrow().calls.clone()
        }

        fn take_calls(&mut self) -> Vec<String> {
            std::mem::take(&mut self.fake.borrow_mut().calls)
        }

        fn advance(&self, secs: i64) {
            self.clock.set(self.clock.get() + secs * 1000);
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn plays_the_session_and_follows_pause_resume_seek_volume_and_stop() {
        let mut r = Rig::new();
        r.tick();
        assert_eq!(
            r.take_calls(),
            ["volume 1"],
            "an idle session plays nothing"
        );

        r.lib.play_track(&r.id("a")).unwrap();
        r.tick();
        assert_eq!(r.take_calls(), ["start a.wav @0"]);

        r.lib.pause().unwrap();
        r.tick();
        r.lib.resume().unwrap();
        r.tick();
        r.lib.seek(Duration::from_millis(500)).unwrap();
        r.tick();
        r.lib.set_volume(0.4).unwrap();
        r.tick();
        r.lib.stop().unwrap();
        r.tick();
        assert_eq!(
            r.take_calls(),
            ["pause", "resume", "seek 500", "volume 0.4", "stop"]
        );
    }

    #[test]
    fn a_finished_track_is_reported_to_the_engine_and_the_next_one_starts() {
        let mut r = Rig::new();
        r.lib
            .queue_add(&[r.id("a"), r.id("b")], None, false)
            .unwrap();
        r.lib.next().unwrap();
        r.tick();
        r.take_calls();

        r.fake.borrow_mut().finished = true;
        r.tick(); // reports the end; the engine moves to b
        r.tick(); // follows it
        assert_eq!(r.take_calls(), ["start b.wav @0"]);

        r.fake.borrow_mut().finished = true;
        r.tick();
        r.tick();
        assert_eq!(
            r.take_calls(),
            ["stop"],
            "the end of the queue stops the audio"
        );
        assert_eq!(r.lib.player_state().unwrap().status, PlayStatus::Stopped);
    }

    #[test]
    fn repeat_song_starts_the_same_file_again() {
        let mut r = Rig::new();
        r.lib.play_track(&r.id("a")).unwrap();
        r.lib.set_repeat(RepeatMode::Song).unwrap();
        r.tick();
        r.take_calls();
        r.fake.borrow_mut().finished = true;
        r.tick();
        r.tick();
        assert_eq!(r.take_calls(), ["start a.wav @0"]);
    }

    #[test]
    fn a_host_started_late_picks_up_where_the_session_is() {
        let mut r = Rig::new();
        r.lib.play_track(&r.id("a")).unwrap();
        r.advance(40);
        r.tick();
        assert_eq!(
            r.take_calls(),
            ["volume 1", "start a.wav @40000"][..],
            "resumes in place, not from zero"
        );
    }

    #[test]
    fn a_paused_session_is_loaded_but_not_played() {
        let mut r = Rig::new();
        r.lib.play_track(&r.id("b")).unwrap();
        r.lib.pause().unwrap();
        r.tick();
        assert_eq!(r.take_calls(), ["volume 1", "start b.wav @0", "pause"]);
    }

    #[test]
    fn the_real_position_is_published_for_other_commands() {
        let mut r = Rig::new();
        r.lib.play_track(&r.id("a")).unwrap();
        r.tick();
        r.advance(10);
        r.fake.borrow_mut().position = Duration::from_millis(9_700);
        r.tick();
        assert_eq!(
            r.lib.session().unwrap().position,
            Duration::from_millis(9_700),
            "the host's figure wins"
        );
    }

    #[test]
    fn the_sleep_timer_stops_the_audio_on_time() {
        let mut r = Rig::new();
        r.lib.play_track(&r.id("a")).unwrap();
        r.tick();
        r.take_calls();
        r.lib
            .set_sleep(SleepSetting::In {
                delay: Duration::from_secs(30),
                stop_after: false,
            })
            .unwrap();
        r.tick(); // the host sees the timer being set, as it would within 100 ms
        r.advance(31);
        let events = r.tick();
        assert_eq!(r.take_calls(), ["stop"]);
        assert!(events.contains(&PlayerEvent::SleepStopped), "{events:?}");
    }

    fn add_unplayable(r: &mut Rig, title: &str) -> String {
        let t = ScannedTrack {
            external_id: format!("{title}.ogg"),
            metadata: weave_core::Metadata {
                title: title.into(),
                ..Default::default()
            },
        };
        r.lib.import_scanned("elsewhere", &[t]).unwrap();
        format!("unknown-{title}-elsewhere")
    }

    #[test]
    fn a_track_that_cannot_be_played_is_skipped_with_an_error_event() {
        let mut r = Rig::new();
        let bad = add_unplayable(&mut r, "x");
        r.lib.queue_add(&[bad, r.id("a")], None, false).unwrap();
        r.lib.next().unwrap();
        let events = r.tick();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, PlayerEvent::Error(m) if m.contains("cannot play"))),
            "{events:?}"
        );
        r.tick();
        assert!(
            r.calls().contains(&"start a.wav @0".to_string()),
            "{:?}",
            r.calls()
        );
    }

    #[test]
    fn a_queue_with_nothing_playable_stops_instead_of_looping() {
        let mut r = Rig::new();
        let ids = [add_unplayable(&mut r, "x"), add_unplayable(&mut r, "y")];
        r.lib.queue_add(&ids, None, false).unwrap();
        r.lib.set_repeat(RepeatMode::Playlist).unwrap();
        r.lib.next().unwrap();
        for _ in 0..10 {
            r.tick();
        }
        assert_eq!(r.lib.player_state().unwrap().status, PlayStatus::Stopped);
    }

    #[test]
    fn an_output_that_fails_to_start_is_an_error_event_not_a_crash() {
        let mut r = Rig::new();
        r.fake.borrow_mut().fail_start = true;
        r.lib.play_track(&r.id("a")).unwrap();
        let events = r.tick();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, PlayerEvent::Error(m) if m.contains("device went away"))),
            "{events:?}"
        );
    }

    #[test]
    fn a_host_without_audio_follows_the_session_without_playing() {
        let mut r = Rig::new();
        let mut quiet = Host::new(None);
        r.lib.play_track(&r.id("a")).unwrap();
        quiet.tick(&mut r.lib).unwrap();
        r.advance(100);
        quiet.tick(&mut r.lib).unwrap();
        assert!(r.calls().is_empty());
        assert_eq!(
            r.lib.player_state().unwrap().status,
            PlayStatus::Playing,
            "nothing ends a track without audio"
        );
    }
}
