//! Showing lyrics: a static listing, and `--follow` which prints each line as it comes up.

use std::time::{Duration, Instant};

use weave::Library;
use weave::core::{Lyrics, PlayStatus, Track};

use crate::Failure;
use crate::output::print_json;

fn clock(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

/// One line as shown on screen: `[1:05] text` for synchronized lyrics, plain text otherwise.
pub fn render_line(at_ms: Option<u64>, text: &str) -> String {
    match at_ms {
        Some(ms) => format!("[{}] {text}", clock(ms)),
        None => text.to_string(),
    }
}

/// The whole lyrics as text; `current` (if any) is marked with an arrow.
pub fn render(lyrics: &Lyrics, current: Option<usize>) -> Vec<String> {
    let marker = |i: usize| if Some(i) == current { "▶ " } else { "  " };
    lyrics
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let line = render_line(l.at_ms, &l.text);
            if lyrics.synced {
                format!("{}{line}", marker(i))
            } else {
                line
            }
        })
        .collect()
}

pub fn print(
    json: bool,
    track: &Track,
    lyrics: &Lyrics,
    position_ms: Option<u64>,
) -> Result<(), Failure> {
    let current = position_ms.and_then(|p| lyrics.current_line(p));
    if json {
        return print_json(&serde_json::json!({
            "track": track.id,
            "title": track.metadata.title,
            "synced": lyrics.synced,
            "current": current,
            "lines": lyrics.lines,
        }));
    }
    for line in render(lyrics, current) {
        outln!("{line}");
    }
    Ok(())
}

/// Follow the session and print each synchronized line when it begins. Runs until
/// interrupted (or `run_for`).
pub fn follow(lib: &mut Library, json: bool, run_for: Option<Duration>) -> Result<(), Failure> {
    let started = Instant::now();
    let mut loaded: Option<(String, Option<Lyrics>)> = None;
    let mut last_line: Option<usize> = None;
    let mut announced_plain = false;
    loop {
        let view = lib.session()?;
        match view.current() {
            Some(track) if view.state.status != PlayStatus::Stopped => {
                // (Re)load when the track changes or restarts.
                let key = format!("{}#{}", track.id, view.state.start_seq);
                if loaded.as_ref().map(|(k, _)| k) != Some(&key) {
                    let lyrics = lib.lyrics(&track.id.0)?;
                    if !json {
                        outln!("♪ {}", track.metadata.title);
                    }
                    if lyrics.is_none() && !json {
                        outln!("  (no lyrics)");
                    }
                    loaded = Some((key, lyrics));
                    last_line = None;
                    announced_plain = false;
                }
                if let Some((_, Some(l))) = &loaded {
                    if l.synced {
                        let now = l.current_line(view.position.as_millis() as u64);
                        if now != last_line {
                            last_line = now;
                            if let Some(i) = now {
                                let line = &l.lines[i];
                                if json {
                                    outln!(
                                        "{}",
                                        serde_json::json!({
                                            "track": track.id, "at_ms": line.at_ms, "text": line.text,
                                        })
                                    );
                                } else if !line.text.is_empty() {
                                    outln!("  {}", line.text);
                                }
                            }
                        }
                    } else if !announced_plain {
                        announced_plain = true;
                        if !json {
                            outln!(
                                "  (these lyrics are not synchronized; use `lyrics` to read them)"
                            );
                        }
                    }
                }
            }
            _ => loaded = None,
        }
        if run_for.is_some_and(|limit| started.elapsed() >= limit) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synced_lines_show_a_timestamp_and_mark_the_current_one() {
        let l = Lyrics::parse("[00:05.00]a\n[01:05.00]b").unwrap();
        assert_eq!(render(&l, Some(1)), ["  [0:05] a", "▶ [1:05] b"]);
        assert_eq!(render(&l, None), ["  [0:05] a", "  [1:05] b"]);
    }

    #[test]
    fn plain_lyrics_are_shown_as_they_are() {
        let l = Lyrics::parse("one\ntwo").unwrap();
        assert_eq!(render(&l, Some(0)), ["one", "two"]);
    }
}
