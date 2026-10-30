//! Human-readable alternative track IDs: `artist[-album]-song-source`.
//!
//! A slug only ever contains lowercase Unicode letters/digits and `-`, so it is
//! safe in URLs, shells and SQL literals (queries are parameterized regardless).

const MAX_SEGMENT_CHARS: usize = 48;

/// Rules:
/// - lowercase (Unicode-aware); letters and digits of any script are kept as-is
/// - apostrophes and backticks are dropped, so `Don't` → `dont`
/// - `&` becomes `and`
/// - every other run of characters (spaces, quotes, punctuation, symbols) becomes one `-`
/// - no leading/trailing `-`; capped at 48 characters
pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut pending_sep = false;
    let push = |out: &mut String, c: char, pending: &mut bool| {
        if *pending && !out.is_empty() {
            out.push('-');
        }
        *pending = false;
        out.push(c);
    };
    for c in input.chars() {
        match c {
            '\'' | '’' | '‘' | '`' | '´' => {}
            '&' => {
                pending_sep = true;
                for a in "and".chars() {
                    push(&mut out, a, &mut pending_sep);
                }
                pending_sep = true;
            }
            c if c.is_alphanumeric() => {
                for lc in c.to_lowercase().filter(|l| l.is_alphanumeric()) {
                    push(&mut out, lc, &mut pending_sep);
                }
            }
            _ => pending_sep = true,
        }
    }
    let truncated: String = out.chars().take(MAX_SEGMENT_CHARS).collect();
    truncated.trim_end_matches('-').to_string()
}

/// `artist[-album]-song-source`. Unknown artist → `unknown`, unusable title →
/// `untitled`; the album is left out when absent. Uniqueness is the caller's job.
pub fn track_slug(artist: Option<&str>, album: Option<&str>, title: &str, source: &str) -> String {
    let or = |s: String, fallback: &str| if s.is_empty() { fallback.to_string() } else { s };
    let mut parts = vec![or(slugify(artist.unwrap_or("")), "unknown")];
    let album = slugify(album.unwrap_or(""));
    if !album.is_empty() {
        parts.push(album);
    }
    parts.push(or(slugify(title), "untitled"));
    parts.push(or(slugify(source), "unknown"));
    parts.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_rules() {
        assert_eq!(slugify("Romeo Santos"), "romeo-santos");
        assert_eq!(slugify("Don't Stop"), "dont-stop");
        assert_eq!(slugify("Rock & Roll"), "rock-and-roll");
        assert_eq!(slugify("  --Hello,   World!--  "), "hello-world");
        assert_eq!(slugify("AC/DC"), "ac-dc");
        assert_eq!(slugify("x'; DROP TABLE tracks;--"), "x-drop-table-tracks");
        assert_eq!(slugify("\"Quoted\""), "quoted");
    }

    #[test]
    fn keeps_unicode_letters() {
        assert_eq!(slugify("Édith Piaf"), "édith-piaf");
        assert_eq!(slugify("日本語 ソング"), "日本語-ソング");
    }

    #[test]
    fn empty_and_symbol_only() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("???"), "");
    }

    #[test]
    fn truncates_without_trailing_dash() {
        let s = slugify(&"ab ".repeat(40));
        assert!(s.chars().count() <= 48 && !s.ends_with('-'));
    }

    #[test]
    fn full_slug_with_fallbacks() {
        assert_eq!(
            track_slug(Some("Romeo Santos"), Some("Fórmula, Vol. 2"), "Propuesta Indecente", "local"),
            "romeo-santos-fórmula-vol-2-propuesta-indecente-local"
        );
        assert_eq!(track_slug(None, None, "Song", "local"), "unknown-song-local");
        assert_eq!(track_slug(Some("!!"), Some(""), "???", "Local"), "unknown-untitled-local");
    }
}
