//! The D-Bus side: a thread running mpris-server's `Player`.

use std::sync::mpsc;
use std::time::Duration;

use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};
use weave_core::{PlayStatus, RepeatMode};

use super::{Remote, Snapshot, TrackInfo};

pub struct Mpris {
    snapshots: async_channel::Sender<Snapshot>,
    remote: mpsc::Receiver<Remote>,
}

impl Mpris {
    /// Claim `org.mpris.MediaPlayer2.<bus_name>` on the session bus. Fails (with the reason)
    /// when there is no session bus or the name is taken.
    pub fn start(bus_name: &str) -> Result<Mpris, String> {
        let (snapshots, snapshot_rx) = async_channel::unbounded::<Snapshot>();
        let (remote_tx, remote) = mpsc::channel::<Remote>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let bus_name = bus_name.to_string();

        std::thread::Builder::new()
            .name("mpris".into())
            .spawn(move || {
                async_io::block_on(async move {
                    let player = match build(&bus_name, &remote_tx).await {
                        Ok(p) => p,
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    let _ = ready_tx.send(Ok(()));
                    let updates = async {
                        let mut previous: Option<Snapshot> = None;
                        while let Ok(snapshot) = snapshot_rx.recv().await {
                            apply(&player, previous.as_ref(), &snapshot).await;
                            previous = Some(snapshot);
                        }
                    };
                    // `updates` ends when the host drops its sender, which ends the thread.
                    futures_lite::future::or(async { player.run().await }, updates).await;
                })
            })
            .map_err(|e| e.to_string())?;

        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "timed out connecting to the session bus".to_string())??;
        Ok(Mpris { snapshots, remote })
    }

    pub fn send(&self, snapshot: Snapshot) {
        let _ = self.snapshots.send_blocking(snapshot);
    }

    pub fn try_recv(&self) -> Option<Remote> {
        self.remote.try_recv().ok()
    }
}

/// Registering a name another player already owns does not fail: the request is queued and
/// the desktop keeps talking to the first owner. Look first, so the host can say so.
async fn ensure_name_is_free(bus_name: &str) -> Result<(), String> {
    use mpris_server::zbus::{self, fdo::DBusProxy, names::BusName};
    let full = format!("org.mpris.MediaPlayer2.{bus_name}");
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;
    let proxy = DBusProxy::new(&connection)
        .await
        .map_err(|e| e.to_string())?;
    let name = BusName::try_from(full.as_str()).map_err(|e| e.to_string())?;
    if proxy
        .name_has_owner(name)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(format!(
            "{full} is already owned by another player (is another weave host running?)"
        ));
    }
    Ok(())
}

async fn build(bus_name: &str, tx: &mpsc::Sender<Remote>) -> Result<Player, String> {
    ensure_name_is_free(bus_name).await?;
    let player = Player::builder(bus_name)
        .identity("Weave")
        .can_quit(true)
        .can_play(false)
        .can_pause(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_seek(true)
        .can_control(true)
        .build()
        .await
        .map_err(|e| e.to_string())?;

    let send = |remote: Remote| {
        let tx = tx.clone();
        move || {
            let _ = tx.send(remote.clone());
        }
    };
    let (play, pause, play_pause, stop) = (
        send(Remote::Play),
        send(Remote::Pause),
        send(Remote::PlayPause),
        send(Remote::Stop),
    );
    player.connect_play(move |_| play());
    player.connect_pause(move |_| pause());
    player.connect_play_pause(move |_| play_pause());
    player.connect_stop(move |_| stop());
    let (next, previous, quit) = (
        send(Remote::Next),
        send(Remote::Previous),
        send(Remote::Quit),
    );
    player.connect_next(move |_| next());
    player.connect_previous(move |_| previous());
    player.connect_quit(move |_| quit());

    let t = tx.clone();
    player.connect_seek(move |_, offset| {
        let _ = t.send(Remote::SeekBy(offset.as_millis()));
    });
    let t = tx.clone();
    player.connect_set_position(move |_, _track, position| {
        let _ = t.send(Remote::SetPosition(position.as_millis().max(0) as u64));
    });
    let t = tx.clone();
    player.connect_set_volume(move |_, volume| {
        let _ = t.send(Remote::SetVolume(volume.clamp(0.0, 1.0) as f32));
    });
    let t = tx.clone();
    player.connect_set_shuffle(move |_, on| {
        let _ = t.send(Remote::SetShuffle(on));
    });
    let t = tx.clone();
    player.connect_set_loop_status(move |_, status| {
        let _ = t.send(Remote::SetRepeat(repeat_from(status)));
    });
    Ok(player)
}

fn playback_status(s: PlayStatus) -> PlaybackStatus {
    match s {
        PlayStatus::Stopped => PlaybackStatus::Stopped,
        PlayStatus::Playing => PlaybackStatus::Playing,
        PlayStatus::Paused => PlaybackStatus::Paused,
    }
}

fn loop_status(r: RepeatMode) -> LoopStatus {
    match r {
        RepeatMode::Off => LoopStatus::None,
        RepeatMode::Song => LoopStatus::Track,
        RepeatMode::Playlist => LoopStatus::Playlist,
    }
}

fn repeat_from(l: LoopStatus) -> RepeatMode {
    match l {
        LoopStatus::None => RepeatMode::Off,
        LoopStatus::Track => RepeatMode::Song,
        LoopStatus::Playlist => RepeatMode::Playlist,
    }
}

fn metadata(t: &TrackInfo) -> Metadata {
    let mut m = Metadata::builder().title(t.title.clone());
    // D-Bus object paths only allow [A-Za-z0-9_/], so the id is a counter, not the slug.
    if let Ok(id) = TrackId::try_from(format!("/org/weave/track/{}", t.key)) {
        m = m.trackid(id);
    }
    if let Some(a) = &t.artist {
        m = m.artist([a.clone()]);
    }
    if let Some(a) = &t.album {
        m = m.album(a.clone());
    }
    if let Some(ms) = t.length_ms {
        m = m.length(Time::from_millis(ms as i64));
    }
    if let Some(l) = &t.lyrics {
        m = m.lyrics(l.clone());
    }
    m.build()
}

/// Push what differs from `previous` to D-Bus.
async fn apply(player: &Player, previous: Option<&Snapshot>, s: &Snapshot) {
    let changed = |f: fn(&Snapshot) -> bool| previous.is_none_or(|p| f(p) != f(s));
    player.set_position(Time::from_millis(s.position_ms as i64));

    if previous.is_none_or(|p| p.track != s.track) {
        let meta = s.track.as_ref().map_or_else(Metadata::new, metadata);
        let _ = player.set_metadata(meta).await;
    }
    let has_track = s.track.is_some();
    if changed(|x| x.track.is_some()) {
        let _ = player.set_can_play(has_track).await;
        let _ = player.set_can_go_next(has_track).await;
        let _ = player.set_can_go_previous(has_track).await;
        let _ = player.set_can_seek(has_track).await;
    }
    if previous.is_none_or(|p| p.status != s.status) {
        let _ = player.set_playback_status(playback_status(s.status)).await;
    }
    if previous.is_none_or(|p| p.volume != s.volume) {
        let _ = player.set_volume(f64::from(s.volume)).await;
    }
    if changed(|x| x.shuffle) {
        let _ = player.set_shuffle(s.shuffle).await;
    }
    if previous.is_none_or(|p| p.repeat != s.repeat) {
        let _ = player.set_loop_status(loop_status(s.repeat)).await;
    }
    if previous.is_some_and(|p| p.seek_seq != s.seek_seq || p.track != s.track) {
        let _ = player.seeked(Time::from_millis(s.position_ms as i64)).await;
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Instant;

    use super::*;

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
        let Ok(first) = Mpris::start(&name) else {
            eprintln!("no session bus; skipping");
            return;
        };
        let err = Mpris::start(&name).err().expect("the name is taken");
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
        let Ok(mpris) = Mpris::start(&name) else {
            eprintln!("no session bus; skipping");
            return;
        };
        let service = format!("org.mpris.MediaPlayer2.{name}");
        let path = "/org/mpris/MediaPlayer2";
        let player = "org.mpris.MediaPlayer2.Player";
        if busctl(&["status", &service]).is_none() {
            eprintln!("busctl missing; skipping");
            return;
        }

        mpris.send(Snapshot {
            status: PlayStatus::Playing,
            track: Some(TrackInfo {
                key: 7,
                title: "Song".into(),
                artist: Some("Band".into()),
                album: None,
                length_ms: Some(10_000),
                lyrics: Some("la la".into()),
            }),
            position_ms: 2_500,
            volume: 0.5,
            shuffle: true,
            repeat: RepeatMode::Playlist,
            seek_seq: 0,
        });

        let get = |prop: &str| {
            busctl(&["get-property", &service, path, player, prop]).unwrap_or_default()
        };
        assert!(
            wait_for(|| get("PlaybackStatus").contains("Playing")),
            "status never showed"
        );
        assert!(get("Metadata").contains("Song") && get("Metadata").contains("Band"));
        assert!(
            get("Metadata").contains("/org/weave/track/7") && get("Metadata").contains("la la")
        );
        assert!(get("Shuffle").contains("true"));
        assert!(get("LoopStatus").contains("Playlist"));
        assert!(get("Volume").contains("0.5"));
        assert!(get("Position").contains("2500000"), "{}", get("Position"));

        // Commands arrive as `Remote`s.
        busctl(&["call", &service, path, player, "PlayPause"]).unwrap();
        busctl(&["call", &service, path, player, "Next"]).unwrap();
        busctl(&["call", &service, path, player, "Seek", "x", "5000000"]).unwrap();
        let mut got = Vec::new();
        assert!(
            wait_for(|| {
                while let Some(r) = mpris.try_recv() {
                    got.push(r);
                }
                got.len() >= 3
            }),
            "{got:?}"
        );
        assert_eq!(
            got,
            [Remote::PlayPause, Remote::Next, Remote::SeekBy(5_000)]
        );

        busctl(&["set-property", &service, path, player, "Volume", "d", "0.8"]).unwrap();
        assert!(wait_for(|| mpris.try_recv() == Some(Remote::SetVolume(0.8))));

        drop(mpris);
        assert!(
            wait_for(|| busctl(&["status", &service]).is_none()),
            "the name is released on drop"
        );
    }
}
