//! The library used the way a player embedding it would: through the public API only.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use weave_player_lib::core::{Error, Metadata, PlayStatus, ScannedTrack, SleepSetting};
use weave_player_lib::{Audio, Library, Player, PlayerEvent, PlayerOptions};

fn temp_db(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "weave-embed-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("library.db")
}

fn cleanup(db: &Path) {
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// Tracks without files, enough to drive the session.
fn library_with_fake_tracks(db: &Path, titles: &[&str]) -> Library {
    let mut lib = Library::open(db).unwrap();
    let tracks: Vec<_> = titles
        .iter()
        .map(|t| ScannedTrack {
            path: (*t).into(),
            metadata: Metadata {
                title: (*t).into(),
                artist: Some("x".into()),
                duration_ms: Some(180_000),
                ..Default::default()
            },
        })
        .collect();
    lib.import_scanned(&tracks).unwrap();
    lib
}

fn id(title: &str) -> String {
    format!("x-{title}")
}

/// The next event matching `want`, or a failure after `secs`.
fn wait_for(player: &Player, secs: u64, mut want: impl FnMut(&PlayerEvent) -> bool) -> PlayerEvent {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match player.events().recv_timeout(left) {
            Ok(e) if want(&e) => return e,
            Ok(_) => {}
            Err(e) => panic!("no matching event in {secs}s: {e}"),
        }
    }
}

#[test]
fn the_player_follows_what_another_handle_does_to_the_session() {
    let db = temp_db("follow");
    let mut lib = library_with_fake_tracks(&db, &["a", "b"]);
    let player = Player::start(&db, PlayerOptions::silent()).unwrap();

    lib.queue_add(&[id("a"), id("b")], None, false).unwrap();
    lib.next().unwrap();
    let e = wait_for(&player, 3, |e| {
        matches!(e, PlayerEvent::TrackStarted { .. })
    });
    assert_eq!(e.to_string(), "now on \"a\"");
    assert_eq!(e.name(), "track");
    assert_eq!(e.track(), Some("x-a"));

    lib.pause().unwrap();
    wait_for(&player, 3, |e| {
        *e == PlayerEvent::StatusChanged(PlayStatus::Paused)
    });
    lib.resume().unwrap();
    wait_for(&player, 3, |e| {
        *e == PlayerEvent::StatusChanged(PlayStatus::Playing)
    });
    drop(player);
    cleanup(&db);
}

#[test]
fn only_one_player_runs_per_library_and_stopping_frees_it() {
    let db = temp_db("lock");
    let _lib = library_with_fake_tracks(&db, &["a"]);
    assert!(!Player::is_running(&db));

    let first = Player::start(&db, PlayerOptions::silent()).unwrap();
    assert!(Player::is_running(&db));
    let err = Player::start(&db, PlayerOptions::silent())
        .err()
        .expect("refused");
    assert!(matches!(err, Error::AlreadyExists(_)), "{err}");
    assert!(
        Player::is_running(&db),
        "the refusal did not disturb the first one"
    );

    first.stop();
    assert!(!Player::is_running(&db));
    Player::start(&db, PlayerOptions::silent()).unwrap().stop();
    cleanup(&db);
}

#[test]
fn dropping_the_player_stops_it() {
    let db = temp_db("drop");
    let _lib = library_with_fake_tracks(&db, &["a"]);
    drop(Player::start(&db, PlayerOptions::silent()).unwrap());
    assert!(!Player::is_running(&db));
    cleanup(&db);
}

#[test]
fn a_quit_request_ends_the_player_and_closes_its_event_channel() {
    let db = temp_db("quit");
    let mut lib = library_with_fake_tracks(&db, &["a"]);
    let player = Player::start(&db, PlayerOptions::silent()).unwrap();
    assert!(!player.is_finished());

    lib.request_host_quit().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !player.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(player.is_finished());
    assert!(player.events().iter().all(|_| true), "the channel closes");
    assert!(
        !lib.host_quit_requested().unwrap(),
        "the request was consumed"
    );
    drop(player);
    assert!(!Player::is_running(&db));
    cleanup(&db);
}

#[test]
fn an_old_quit_request_does_not_kill_a_player_that_starts_later() {
    let db = temp_db("stale-quit");
    let mut lib = library_with_fake_tracks(&db, &["a"]);
    lib.request_host_quit().unwrap();
    let player = Player::start(&db, PlayerOptions::silent()).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(!player.is_finished());
    drop(player);
    cleanup(&db);
}

#[test]
fn the_sleep_timer_is_enforced_on_time_by_the_embedded_player() {
    let db = temp_db("sleep");
    let mut lib = library_with_fake_tracks(&db, &["a"]);
    let player = Player::start(&db, PlayerOptions::silent()).unwrap();
    lib.play_track(&id("a")).unwrap();
    wait_for(&player, 3, |e| {
        matches!(e, PlayerEvent::TrackStarted { .. })
    });

    lib.set_sleep(SleepSetting::In {
        delay: Duration::from_millis(800),
        stop_after: false,
    })
    .unwrap();
    let started = Instant::now();
    wait_for(&player, 5, |e| *e == PlayerEvent::SleepStopped);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "it fired near the deadline"
    );
    assert_eq!(lib.session().unwrap().state.status, PlayStatus::Stopped);
    drop(player);
    cleanup(&db);
}

struct FailingOutput;

#[test]
fn a_failing_audio_output_fails_start_and_leaves_no_lock_behind() {
    let db = temp_db("bad-audio");
    let _lib = library_with_fake_tracks(&db, &["a"]);
    let options = PlayerOptions {
        audio: Audio::Custom(Box::new(|| {
            let _ = FailingOutput;
            Err(Error::Player("no sound card".into()))
        })),
        ..PlayerOptions::silent()
    };
    let err = Player::start(&db, options).err().expect("start fails");
    assert!(err.to_string().contains("no sound card"), "{err}");
    assert!(!Player::is_running(&db), "the lock was released");
    Player::start(&db, PlayerOptions::silent()).unwrap().stop();
    cleanup(&db);
}

#[test]
fn a_library_that_cannot_be_opened_fails_start() {
    let db = temp_db("bad-db");
    std::fs::write(&db, b"this is not a database").unwrap();
    assert!(Player::start(&db, PlayerOptions::silent()).is_err());
    assert!(!Player::is_running(&db));
    cleanup(&db);
}

#[test]
fn a_remote_only_instance_cannot_start_a_player_until_playback_is_allowed() {
    let db = temp_db("remote-only");
    let mut lib = library_with_fake_tracks(&db, &["a"]);
    lib.set_playback_allowed(false).unwrap();
    let err = Player::start(&db, PlayerOptions::silent()).err().unwrap();
    assert!(err.to_string().contains("remote-only"), "{err}");
    assert!(!Player::is_running(&db), "no lock is left behind");
    lib.set_playback_allowed(true).unwrap();
    assert!(Player::start(&db, PlayerOptions::silent()).is_ok());
    cleanup(&db);
}

// --- with sound: needs the `audio` feature because only then are local files playable ----

#[cfg(feature = "audio")]
mod with_custom_output {
    use std::sync::{Arc, Mutex};

    use weave_player_lib::core::{Playback, Result};

    use super::*;

    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        finished: bool,
    }

    struct Recording(Arc<Mutex<Log>>);

    impl Playback for Recording {
        fn start(&mut self, source: &str, from: Duration) -> Result<()> {
            let file = Path::new(source)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let mut log = self.0.lock().unwrap();
            log.calls
                .push(format!("start {file} @{}", from.as_millis()));
            log.finished = false;
            Ok(())
        }
        fn pause(&mut self) {
            self.0.lock().unwrap().calls.push("pause".into());
        }
        fn resume(&mut self) {
            self.0.lock().unwrap().calls.push("resume".into());
        }
        fn stop(&mut self) {
            self.0.lock().unwrap().calls.push("stop".into());
        }
        fn seek(&mut self, to: Duration) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(format!("seek {}", to.as_millis()));
            Ok(())
        }
        fn position(&self) -> Duration {
            Duration::ZERO
        }
        fn finished(&self) -> bool {
            self.0.lock().unwrap().finished
        }
        fn set_volume(&mut self, volume: f32) {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(format!("volume {volume}"));
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

    fn calls(log: &Arc<Mutex<Log>>) -> Vec<String> {
        log.lock().unwrap().calls.clone()
    }

    fn wait_until(secs: u64, log: &Arc<Mutex<Log>>, mut ok: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while !ok() {
            assert!(
                Instant::now() < deadline,
                "condition not reached in {secs}s; the output saw {:?}",
                calls(log)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn an_embedder_supplies_its_own_output_and_the_player_drives_it() {
        let db = temp_db("custom");
        let music = db.parent().unwrap().join("music");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::write(music.join("one.wav"), wav()).unwrap();
        std::fs::write(music.join("two.wav"), wav()).unwrap();
        let mut lib = Library::open(&db).unwrap();
        assert_eq!(lib.import(&music).unwrap().added, 2);

        let log = Arc::new(Mutex::new(Log::default()));
        let sink = log.clone();
        let options = PlayerOptions {
            audio: Audio::Custom(Box::new(move || {
                Ok(Box::new(Recording(sink)) as Box<dyn Playback>)
            })),
            ..PlayerOptions::silent()
        };
        let player = Player::start(&db, options).unwrap();

        lib.queue_add(&["unknown-one".into(), "unknown-two".into()], None, false)
            .unwrap();
        lib.next().unwrap();
        wait_until(3, &log, || {
            calls(&log).contains(&"start one.wav @0".to_string())
        });

        lib.set_volume(0.5).unwrap();
        wait_until(3, &log, || calls(&log).contains(&"volume 0.5".to_string()));
        // changes made within one tick are coalesced, so wait for each to be seen
        lib.pause().unwrap();
        wait_until(3, &log, || calls(&log).contains(&"pause".to_string()));
        lib.resume().unwrap();
        wait_until(3, &log, || calls(&log).contains(&"resume".to_string()));

        // the output reports the end of the track: the player advances the session itself
        log.lock().unwrap().finished = true;
        wait_until(3, &log, || {
            calls(&log).contains(&"start two.wav @0".to_string())
        });
        assert_eq!(
            lib.session().unwrap().current().unwrap().metadata.title,
            "two"
        );

        drop(player);
        cleanup(&db);
    }
}
