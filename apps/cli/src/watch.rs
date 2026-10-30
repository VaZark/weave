//! `weave-player watch`: an optional foreground host for the playback session.
//!
//! Short-lived commands cannot hold a timer, so this keeps one: it sleeps until the
//! sleep timer is due (or the check interval, whichever is sooner), then asks the
//! engine to apply it. Other commands can change the session at any time, so it also
//! re-reads it every interval and reports what changed. Run it in a terminal, or in
//! the background (`weave-player watch &`, a systemd user service, ...). Nothing is
//! started automatically. The audio player in 0.a4 will be the same kind of host.

use std::time::{Duration, Instant};

use weave_core::PlayStatus;
use weave_engine::{Library, SessionView};

use crate::Failure;

/// Never spin: the shortest wait between checks.
const MIN_WAIT: Duration = Duration::from_millis(50);

pub fn run(
    lib: &mut Library,
    json: bool,
    interval: Duration,
    run_for: Option<Duration>,
) -> Result<(), Failure> {
    let started = Instant::now();
    let mut last = lib.session()?;
    if !json {
        eprintln!("watching the playback session (Ctrl-C to stop)");
    }
    loop {
        let view = lib.session()?;
        for event in events(&last, &view) {
            emit(json, started.elapsed(), &event);
        }
        last = view;

        if run_for.is_some_and(|limit| started.elapsed() >= limit) {
            return Ok(());
        }
        let mut wait = lib.next_wakeup()?.map_or(interval, |d| d.min(interval));
        if let Some(limit) = run_for {
            wait = wait.min(limit.saturating_sub(started.elapsed()));
        }
        std::thread::sleep(wait.max(MIN_WAIT));
    }
}

pub struct Event {
    pub name: &'static str,
    pub text: String,
    pub track: Option<String>,
}

fn label(v: &SessionView) -> String {
    v.current()
        .map_or_else(|| "(nothing)".into(), |t| t.metadata.title.clone())
}

fn track_id(v: &SessionView) -> Option<String> {
    v.current().map(|t| t.id.0.clone())
}

/// What changed between two snapshots of the session.
pub fn events(before: &SessionView, after: &SessionView) -> Vec<Event> {
    let mut out = Vec::new();
    let mut push = |name, text: String| {
        out.push(Event {
            name,
            text,
            track: track_id(after),
        })
    };

    let timer_gone = before.state.sleep.is_some() && after.state.sleep.is_none();
    if timer_gone && after.state.status == PlayStatus::Stopped {
        push("sleep_stopped", "sleep timer: playback stopped".into());
    } else if timer_gone && after.state.stop_after_track {
        push(
            "stop_scheduled",
            format!("sleep timer: stopping after \"{}\"", label(after)),
        );
    } else if !before.state.stop_after_track && after.state.stop_after_track {
        push(
            "stop_scheduled",
            format!("stop scheduled: stopping after \"{}\"", label(after)),
        );
    }
    if track_id(before) != track_id(after) && after.state.current.is_some() {
        push("track", format!("now on \"{}\"", label(after)));
    }
    if before.state.status != after.state.status && !timer_gone {
        push("status", format!("{}", after.state.status));
    }
    out
}

fn emit(json: bool, elapsed: Duration, e: &Event) {
    if json {
        outln!(
            "{}",
            serde_json::json!({
                "at_secs": elapsed.as_secs(),
                "event": e.name,
                "message": e.text,
                "track": e.track,
            })
        );
    } else {
        outln!("[+{}s] {}", elapsed.as_secs(), e.text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_core::{PlayerState, SleepTimer};

    fn view(status: PlayStatus, sleep: bool, stop_after_track: bool) -> SessionView {
        SessionView {
            state: PlayerState {
                status,
                sleep: sleep.then_some(SleepTimer {
                    at_ms: 1,
                    stop_after: false,
                }),
                stop_after_track,
                ..Default::default()
            },
            tracks: Vec::new(),
            sleep_remaining: None,
        }
    }

    fn names(before: &SessionView, after: &SessionView) -> Vec<&'static str> {
        events(before, after).iter().map(|e| e.name).collect()
    }

    #[test]
    fn reports_a_timer_stopping_playback() {
        let a = view(PlayStatus::Playing, true, false);
        let b = view(PlayStatus::Stopped, false, false);
        assert_eq!(names(&a, &b), ["sleep_stopped"]);
    }

    #[test]
    fn reports_a_timer_turning_into_a_scheduled_stop() {
        let a = view(PlayStatus::Playing, true, false);
        let b = view(PlayStatus::Playing, false, true);
        assert_eq!(names(&a, &b), ["stop_scheduled"]);
    }

    #[test]
    fn reports_a_stop_scheduled_by_another_command() {
        let a = view(PlayStatus::Playing, false, false);
        let b = view(PlayStatus::Playing, false, true);
        assert_eq!(names(&a, &b), ["stop_scheduled"]);
    }

    #[test]
    fn reports_plain_status_changes_and_nothing_when_idle() {
        let playing = view(PlayStatus::Playing, false, false);
        let paused = view(PlayStatus::Paused, false, false);
        assert_eq!(names(&playing, &paused), ["status"]);
        assert!(names(&playing, &playing).is_empty());
    }
}
