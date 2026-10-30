//! The CLI's side of the playback host. The player itself lives in the `weave` library; this
//! starts a detached `weave-player host` process when a command leaves the session playing, and
//! runs the library's embedded `Player` in the foreground for `weave-player host`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use weave::core::Error;
use weave::{Audio, Player, PlayerEvent, PlayerOptions};

use crate::Failure;

/// Where a detached host writes what it would print.
pub fn log_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(".host.log");
    PathBuf::from(name)
}

/// Start a detached host for `db` unless one is running. Returns the log file's path when
/// it started one.
pub fn ensure_running(db: &Path) -> std::io::Result<Option<PathBuf>> {
    if Player::is_running(db) {
        return Ok(None);
    }
    let log = log_path(db);
    let out = File::create(&log)?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.arg("--db")
        .arg(db)
        .arg("host")
        .stdin(std::process::Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out);
    #[cfg(unix)]
    {
        // Its own process group, so Ctrl-C in the terminal that started it does not kill it.
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    }
    cmd.spawn()?;
    Ok(Some(log))
}

/// Print one event as a log line (JSON lines with `--json`).
fn emit(json: bool, elapsed: Duration, event: &PlayerEvent) {
    if json {
        outln!(
            "{}",
            serde_json::json!({
                "at_secs": elapsed.as_secs(),
                "event": event.name(),
                "message": event.to_string(),
                "track": event.track(),
            })
        );
    } else {
        outln!("[+{}s] {event}", elapsed.as_secs());
    }
}

/// Run the library's player here until it is asked to quit, `run_for` passes, or the process ends.
pub fn run_foreground(
    db: &Path,
    json: bool,
    no_audio: bool,
    no_media_controls: bool,
    run_for: Option<Duration>,
) -> Result<(), Failure> {
    let mut options = PlayerOptions::default();
    if no_audio {
        options.audio = Audio::Off;
    }
    options.media_controls &= !no_media_controls;

    let player = match Player::start(db, options) {
        Ok(player) => player,
        Err(Error::AlreadyExists(_)) => {
            eprintln!("a playback host is already running");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    eprintln!(
        "playback host started (pid {}, {})",
        std::process::id(),
        if no_audio { "no audio" } else { "audio on" }
    );
    let started = Instant::now();
    loop {
        match player.events().recv_timeout(Duration::from_millis(100)) {
            Ok(event) => emit(json, started.elapsed(), &event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break, // the player ended (a quit request)
        }
        if run_for.is_some_and(|limit| started.elapsed() >= limit) {
            break;
        }
    }
    player.stop();
    eprintln!("playback host stopped");
    Ok(())
}
