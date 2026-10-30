use std::process::Command;
use std::time::Instant;

use super::*;

fn snapshot(track: Option<TrackInfo>) -> Snapshot {
    Snapshot {
        status: PlayStatus::Playing,
        track,
        position_ms: 2_500,
        volume: 0.5,
        shuffle: true,
        repeat: RepeatMode::Playlist,
        seek_seq: 0,
    }
}

fn track() -> TrackInfo {
    TrackInfo {
        key: 7,
        title: "Song".into(),
        artist: Some("Band".into()),
        album: Some("Record".into()),
        length_ms: Some(10_000),
        lyrics: Some("la la".into()),
    }
}

// --- the pure mappings --------------------------------------------------------------------------

#[test]
fn os_requests_become_player_requests() {
    use MediaControlEvent as E;
    use SeekDirection::{Backward, Forward};
    let ms = Duration::from_millis;
    let cases = [
        (E::Play, Remote::Play),
        (E::Pause, Remote::Pause),
        (E::Toggle, Remote::PlayPause),
        (E::Stop, Remote::Stop),
        (E::Next, Remote::Next),
        (E::Previous, Remote::Previous),
        (E::Quit, Remote::Quit),
        (E::SeekBy(Forward, ms(5_000)), Remote::SeekBy(5_000)),
        (E::SeekBy(Backward, ms(2_500)), Remote::SeekBy(-2_500)),
        (E::Seek(Forward), Remote::SeekBy(5_000)),
        (E::Seek(Backward), Remote::SeekBy(-5_000)),
        (E::FastForward, Remote::SeekBy(10_000)),
        (E::Rewind, Remote::SeekBy(-10_000)),
        (
            E::SetPosition(MediaPosition(ms(90_000))),
            Remote::SetPosition(90_000),
        ),
        (E::SetVolume(0.8), Remote::SetVolume(0.8)),
        (E::SetVolume(7.0), Remote::SetVolume(1.0)),
        (E::SetVolume(-1.0), Remote::SetVolume(0.0)),
        (E::SetShuffle(true), Remote::SetShuffle(true)),
        (
            E::SetRepeat(Repeat::Track),
            Remote::SetRepeat(RepeatMode::Song),
        ),
        (
            E::SetRepeat(Repeat::Playlist),
            Remote::SetRepeat(RepeatMode::Playlist),
        ),
        (
            E::SetRepeat(Repeat::None),
            Remote::SetRepeat(RepeatMode::Off),
        ),
    ];
    for (event, want) in cases {
        assert_eq!(remote_from(&event), Some(want), "{event:?}");
    }
}

#[test]
fn requests_with_no_meaning_here_and_nonsense_volumes_are_ignored() {
    use MediaControlEvent as E;
    for event in [
        E::Raise,
        E::OpenUri("file:///x".into()),
        E::SetRate(2.0),
        E::SetFullscreen(true),
        E::ChannelUp,
        E::ChannelDown,
        E::SetVolume(f64::NAN),
    ] {
        assert_eq!(remote_from(&event), None, "{event:?}");
    }
}

#[test]
fn repeat_modes_round_trip() {
    for mode in RepeatMode::ALL {
        assert_eq!(repeat_from(repeat_of(*mode)), *mode);
    }
}

#[test]
fn the_playback_state_carries_the_position_only_with_a_track() {
    let with = snapshot(Some(track()));
    assert_eq!(
        playback_of(&with),
        MediaPlayback::Playing {
            progress: Some(MediaPosition(Duration::from_millis(2_500)))
        }
    );
    let paused = Snapshot {
        status: PlayStatus::Paused,
        ..snapshot(Some(track()))
    };
    assert!(matches!(
        playback_of(&paused),
        MediaPlayback::Paused { progress: Some(_) }
    ));
    let stopped = Snapshot {
        status: PlayStatus::Stopped,
        ..snapshot(Some(track()))
    };
    assert_eq!(playback_of(&stopped), MediaPlayback::Stopped);
    let nothing = snapshot(None);
    assert_eq!(
        playback_of(&nothing),
        MediaPlayback::Playing { progress: None }
    );
}

#[test]
fn track_metadata_maps_to_the_fields_the_os_shows() {
    let m = metadata_of(&track());
    assert_eq!(m.title.as_deref(), Some("Song"));
    assert_eq!(m.artists, Some(vec!["Band".to_string()]));
    assert_eq!(m.artist.as_deref(), Some("Band"));
    assert_eq!(m.album_title.as_deref(), Some("Record"));
    assert_eq!(m.duration, Some(Duration::from_secs(10)));
    assert_eq!(m.lyrics.as_deref(), Some("la la"));
    let bare = metadata_of(&TrackInfo {
        artist: None,
        album: None,
        length_ms: None,
        lyrics: None,
        ..track()
    });
    assert!(
        bare.artists.is_none()
            && bare.album_title.is_none()
            && bare.duration.is_none()
            && bare.lyrics.is_none()
    );
}

// --- against the real session bus ---------------------------------------------------------------

fn busctl(args: &[&str]) -> Option<String> {
    let out = Command::new("busctl")
        .arg("--user")
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn wait_for(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_second_player_with_the_same_name_is_refused_with_a_clear_error() {
    let name = format!("weave_dup_{}", std::process::id());
    let Ok(first) = MediaService::start(&name) else {
        eprintln!("no session bus; skipping");
        return;
    };
    // the first one registers its name on its own thread; wait until it is visible
    let service = format!("org.mpris.MediaPlayer2.{name}");
    if busctl(&["status", &service]).is_none()
        && !wait_for(|| busctl(&["status", &service]).is_some())
    {
        eprintln!("busctl missing or the name never appeared; skipping");
        return;
    }
    let err = MediaService::start(&name).err().expect("the name is taken");
    assert!(
        err.contains("already owned") && err.contains(&name),
        "{err}"
    );
    drop(first);
}

/// Talks to the real session bus with `busctl`; skipped where there is none.
#[test]
fn registers_on_the_bus_shows_the_session_and_forwards_commands() {
    let name = format!("weave_test_{}", std::process::id());
    let Ok(mut media) = MediaService::start(&name) else {
        eprintln!("no session bus; skipping");
        return;
    };
    let service = format!("org.mpris.MediaPlayer2.{name}");
    let path = "/org/mpris/MediaPlayer2";
    let player = "org.mpris.MediaPlayer2.Player";
    if !wait_for(|| busctl(&["status", &service]).is_some()) {
        eprintln!("busctl missing or the name never appeared; skipping");
        return;
    }

    media.send(snapshot(Some(track())));

    let get =
        |prop: &str| busctl(&["get-property", &service, path, player, prop]).unwrap_or_default();
    assert!(
        wait_for(|| get("PlaybackStatus").contains("Playing")),
        "status never showed"
    );
    let metadata = get("Metadata");
    assert!(
        metadata.contains("Song") && metadata.contains("Band") && metadata.contains("Record"),
        "{metadata}"
    );
    assert!(
        metadata.contains("la la"),
        "lyrics (xesam:asText) missing: {metadata}"
    );
    assert!(metadata.contains("10000000"), "length missing: {metadata}");
    assert!(get("Shuffle").contains("true"));
    assert!(get("LoopStatus").contains("Playlist"));
    assert!(get("Volume").contains("0.5"));
    assert!(
        wait_for(|| get("Position").contains("2500000")),
        "{}",
        get("Position")
    );
    assert!(
        get("CanSeek").contains("true"),
        "permissions follow having a track"
    );

    // A change is pushed; no track turns the transport permissions off again.
    media.send(Snapshot {
        status: PlayStatus::Paused,
        ..snapshot(Some(track()))
    });
    assert!(wait_for(|| get("PlaybackStatus").contains("Paused")));
    media.send(Snapshot {
        status: PlayStatus::Stopped,
        ..snapshot(None)
    });
    assert!(
        wait_for(|| get("CanSeek").contains("false")),
        "{}",
        get("CanSeek")
    );

    // Commands arrive as `Remote`s.
    busctl(&["call", &service, path, player, "PlayPause"]).unwrap();
    busctl(&["call", &service, path, player, "Next"]).unwrap();
    busctl(&["call", &service, path, player, "Seek", "x", "5000000"]).unwrap();
    // `--` so busctl does not read the negative amount as an option
    busctl(&[
        "call", &service, path, player, "Seek", "--", "x", "-2000000",
    ])
    .unwrap();
    let mut got = Vec::new();
    assert!(
        wait_for(|| {
            while let Some(r) = media.try_recv() {
                got.push(r);
            }
            got.len() >= 4
        }),
        "{got:?}"
    );
    assert_eq!(
        got,
        [
            Remote::PlayPause,
            Remote::Next,
            Remote::SeekBy(5_000),
            Remote::SeekBy(-2_000)
        ]
    );

    busctl(&["set-property", &service, path, player, "Volume", "d", "0.8"]).unwrap();
    assert!(wait_for(|| media.try_recv() == Some(Remote::SetVolume(0.8))));
    busctl(&["call", &service, path, "org.mpris.MediaPlayer2", "Quit"]).unwrap();
    assert!(wait_for(|| media.try_recv() == Some(Remote::Quit)));

    drop(media);
    assert!(
        wait_for(|| busctl(&["status", &service]).is_none()),
        "the name is released on drop"
    );
}
