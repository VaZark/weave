//! Lyrics, synchronized (LRC) or plain. Pure parsing and lookup; finding the text is a
//! file reader's job.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    /// When the line starts; `None` for plain (unsynchronized) lyrics.
    pub at_ms: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lyrics {
    pub synced: bool,
    /// In playing order; synchronized lyrics are sorted by time.
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    /// Parse LRC text. Text with no timestamps at all becomes plain lyrics; in text that
    /// has some, lines without a timestamp (headers, notes) are dropped. Supports several
    /// timestamps per line, `[offset:+ms]` and enhanced `<mm:ss.xx>` word marks (ignored).
    /// `None` when there is nothing to show.
    pub fn parse(text: &str) -> Option<Lyrics> {
        let text = text.trim_start_matches('\u{feff}');
        let mut offset: i64 = 0;
        let mut synced: Vec<(u64, String)> = Vec::new();
        let mut plain: Vec<String> = Vec::new();

        for raw in text.lines() {
            let line = raw.trim_end();
            let (stamps, rest) = leading_tags(line);
            let mut times = Vec::new();
            let mut is_meta = false;
            for tag in &stamps {
                if let Some(ms) = timestamp_ms(tag) {
                    times.push(ms);
                } else if let Some(v) = tag.strip_prefix("offset:") {
                    offset = v.trim().parse().unwrap_or(0);
                    is_meta = true;
                } else if tag.contains(':') {
                    is_meta = true; // [ar:..], [ti:..], [by:..] and the like
                }
            }
            if times.is_empty() {
                if !is_meta && !line.trim().is_empty() {
                    plain.push(line.trim().to_string());
                }
                continue;
            }
            let words = strip_word_marks(rest).trim().to_string();
            for ms in times {
                synced.push((ms, words.clone()));
            }
        }

        if !synced.is_empty() {
            // A positive offset makes lyrics appear sooner.
            let mut lines: Vec<LyricLine> = synced
                .into_iter()
                .map(|(ms, text)| LyricLine {
                    at_ms: Some((ms as i64 - offset).max(0) as u64),
                    text,
                })
                .collect();
            lines.sort_by_key(|l| l.at_ms); // stable: equal times keep file order
            return Some(Lyrics {
                synced: true,
                lines,
            });
        }
        if plain.is_empty() {
            return None;
        }
        Some(Lyrics {
            synced: false,
            lines: plain
                .into_iter()
                .map(|text| LyricLine { at_ms: None, text })
                .collect(),
        })
    }

    /// Index of the line being sung at `position_ms`: the last line that has started.
    /// `None` before the first line, and always for plain lyrics.
    pub fn current_line(&self, position_ms: u64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        self.lines
            .partition_point(|l| l.at_ms.is_some_and(|at| at <= position_ms))
            .checked_sub(1)
    }

    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Leading `[...]` tags of a line and the text after them.
fn leading_tags(line: &str) -> (Vec<String>, &str) {
    let mut tags = Vec::new();
    let mut rest = line.trim_start();
    while let Some(inner) = rest.strip_prefix('[') {
        let Some(end) = inner.find(']') else { break };
        tags.push(inner[..end].trim().to_string());
        rest = &inner[end + 1..];
    }
    (tags, rest)
}

/// `mm:ss`, `mm:ss.xx`, `mm:ss.xxx` (also `hh:mm:ss.xx`) to milliseconds.
fn timestamp_ms(tag: &str) -> Option<u64> {
    let (clock, fraction) = match tag.split_once('.') {
        Some((c, f)) => (c, Some(f)),
        None => (tag, None),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let mut seconds = 0u64;
    for p in &parts {
        seconds = seconds.checked_mul(60)?.checked_add(p.parse().ok()?)?;
    }
    let millis = match fraction {
        None => 0,
        Some(f) if !f.is_empty() && f.len() <= 3 && f.bytes().all(|b| b.is_ascii_digit()) => {
            f.parse::<u64>().ok()? * 10u64.pow(3 - f.len() as u32)
        }
        Some(_) => return None,
    };
    Some(seconds * 1000 + millis)
}

/// Remove enhanced-LRC word timestamps like `<00:12.30>`.
fn strip_word_marks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        match after.find('>') {
            Some(end) if timestamp_ms(&after[..end]).is_some() => {
                out.push_str(&rest[..start]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(l: &Lyrics) -> Vec<Option<u64>> {
        l.lines.iter().map(|x| x.at_ms).collect()
    }

    fn texts(l: &Lyrics) -> Vec<&str> {
        l.lines.iter().map(|x| x.text.as_str()).collect()
    }

    #[test]
    fn parses_timestamps_in_centis_millis_and_whole_seconds() {
        let l = Lyrics::parse("[00:01.50]a\n[00:02.250]b\n[00:03]c\n[1:00:00.00]d").unwrap();
        assert!(l.synced);
        assert_eq!(
            at(&l),
            [Some(1500), Some(2250), Some(3000), Some(3_600_000)]
        );
        assert_eq!(texts(&l), ["a", "b", "c", "d"]);
    }

    #[test]
    fn several_timestamps_on_one_line_repeat_it_in_order() {
        let l = Lyrics::parse("[00:30.00][00:10.00]chorus\n[00:20.00]verse").unwrap();
        assert_eq!(texts(&l), ["chorus", "verse", "chorus"]);
        assert_eq!(at(&l), [Some(10_000), Some(20_000), Some(30_000)]);
    }

    #[test]
    fn offset_shifts_lyrics_earlier_when_positive_and_clamps_at_zero() {
        let l = Lyrics::parse("[offset:+500]\n[00:00.20]a\n[00:01.00]b").unwrap();
        assert_eq!(at(&l), [Some(0), Some(500)]);
        let l = Lyrics::parse("[offset:-500]\n[00:01.00]a").unwrap();
        assert_eq!(at(&l), [Some(1500)]);
    }

    #[test]
    fn metadata_tags_and_untimed_notes_are_dropped_from_synced_lyrics() {
        let l = Lyrics::parse(
            "\u{feff}[ti:Song]\r\n[ar:Band]\r\nnote without time\r\n[00:01.00]hello\r\n",
        )
        .unwrap();
        assert_eq!(texts(&l), ["hello"]);
    }

    #[test]
    fn enhanced_word_marks_are_stripped_but_other_angle_brackets_stay() {
        let l = Lyrics::parse("[00:01.00]<00:01.00>Hel<00:01.40>lo <3 you").unwrap();
        assert_eq!(texts(&l), ["Hello <3 you"]);
    }

    #[test]
    fn empty_timed_lines_are_kept_as_instrumental_breaks() {
        let l = Lyrics::parse("[00:01.00]a\n[00:05.00]\n[00:09.00]b").unwrap();
        assert_eq!(texts(&l), ["a", "", "b"]);
    }

    #[test]
    fn text_without_timestamps_is_plain() {
        let l = Lyrics::parse("first\n\n  second  \n[ar:Band]").unwrap();
        assert!(!l.synced);
        assert_eq!(texts(&l), ["first", "second"]);
        assert_eq!(at(&l), [None, None]);
        assert_eq!(l.plain_text(), "first\nsecond");
        assert_eq!(l.current_line(10_000), None);
    }

    #[test]
    fn nothing_to_show_is_none() {
        assert_eq!(Lyrics::parse(""), None);
        assert_eq!(Lyrics::parse("\n  \n[ti:x]\n"), None);
    }

    #[test]
    fn malformed_timestamps_are_not_timestamps() {
        for bad in [
            "[0:1:2:3]x",
            "[aa:bb]x",
            "[00:01.]x",
            "[00:01.1234]x",
            "[00:]x",
            "[:01]x",
        ] {
            let l = Lyrics::parse(bad);
            assert!(l.as_ref().is_none_or(|l| !l.synced), "{bad}: {l:?}");
        }
    }

    #[test]
    fn current_line_is_the_last_one_that_has_started() {
        let l = Lyrics::parse("[00:10.00]a\n[00:20.00]b\n[00:30.00]c").unwrap();
        assert_eq!(l.current_line(0), None, "before the first line");
        assert_eq!(l.current_line(9_999), None);
        assert_eq!(l.current_line(10_000), Some(0), "exactly at the timestamp");
        assert_eq!(l.current_line(19_999), Some(0));
        assert_eq!(l.current_line(25_000), Some(1));
        assert_eq!(
            l.current_line(999_999),
            Some(2),
            "after the last line stays on it"
        );
    }
}
