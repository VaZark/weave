//! Human-readable alternative track IDs: `artist[-album]-song`.
//!
//! A slug only ever contains lowercase Unicode letters/digits and `-`, so it is
//! safe in URLs, shells and SQL literals (queries are parameterized regardless).

const MAX_SEGMENT_CHARS: usize = 48;

/// Rules:
/// - lowercase (Unicode-aware); letters and digits of any script are kept as-is
/// - apostrophes and backticks are dropped, so `Don't` → `dont`
/// - `&` becomes `and`
/// - every other run of characters (spaces, quotes, punctuation, symbols) becomes one `-`
/// - no leading/trailing `-`
///
/// No length cap here: lookups normalize whole slugs with this. `track_slug` caps each part.
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
    out
}

fn segment(input: &str) -> String {
    let capped: String = slugify(input).chars().take(MAX_SEGMENT_CHARS).collect();
    capped.trim_end_matches('-').to_string()
}

/// `artist[-album]-song`. Unknown artist → `unknown`, unusable title →
/// `untitled`; the album is left out when absent. Uniqueness is the caller's job.
pub fn track_slug(artist: Option<&str>, album: Option<&str>, title: &str) -> String {
    let or = |s: String, fallback: &str| {
        if s.is_empty() {
            fallback.to_string()
        } else {
            s
        }
    };
    let mut parts = vec![or(segment(artist.unwrap_or("")), "unknown")];
    let album = segment(album.unwrap_or(""));
    if !album.is_empty() {
        parts.push(album);
    }
    parts.push(or(segment(title), "untitled"));
    parts.join("-")
}

/// Slug for an artist name; `unknown` if it has no usable characters.
pub fn artist_slug(name: &str) -> String {
    let s = segment(name);
    if s.is_empty() { "unknown".into() } else { s }
}

/// `artist-album`; `unknown` for a missing artist and `untitled` for an unusable title.
pub fn album_slug(artist: Option<&str>, title: &str) -> String {
    let title = segment(title);
    format!(
        "{}-{}",
        artist_slug(artist.unwrap_or("")),
        if title.is_empty() { "untitled" } else { &title }
    )
}

/// Slug for a playlist name; `playlist` if the name has no usable characters.
/// Uniqueness is the caller's job.
pub fn playlist_slug(name: &str) -> String {
    let s = segment(name);
    if s.is_empty() { "playlist".into() } else { s }
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
    fn parts_are_capped_but_whole_slugs_are_not_truncated() {
        let long = "ab ".repeat(40);
        let slug = track_slug(Some(&long), None, "t");
        let artist = slug.strip_suffix("-t").unwrap();
        assert!(artist.chars().count() <= 48 && !artist.ends_with('-'));
        // Re-normalizing a finished slug must be a no-op, however long.
        assert_eq!(slugify(&slug), slug);
        let long_slug = format!("{}-song", "a".repeat(120));
        assert_eq!(slugify(&long_slug), long_slug);
    }

    #[test]
    fn playlist_slugs() {
        assert_eq!(playlist_slug("Bachata 90s"), "bachata-90s");
        assert_eq!(playlist_slug("Rock & Roll!"), "rock-and-roll");
        assert_eq!(playlist_slug("???"), "playlist");
    }

    #[test]
    fn full_slug_with_fallbacks() {
        assert_eq!(
            track_slug(
                Some("Romeo Santos"),
                Some("Fórmula, Vol. 2"),
                "Propuesta Indecente"
            ),
            "romeo-santos-fórmula-vol-2-propuesta-indecente"
        );
        assert_eq!(track_slug(None, None, "Song"), "unknown-song");
        assert_eq!(track_slug(Some("!!"), Some(""), "???"), "unknown-untitled");
    }
}
