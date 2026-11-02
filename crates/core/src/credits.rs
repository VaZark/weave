//! Reading featured artists out of the text a file gives: `Artist feat. Guest` in the artist
//! field, or `Song (feat. Guest)` in the title.

/// The primary artist and the featured ones, as named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credits {
    pub primary: Option<String>,
    pub featured: Vec<String>,
}

const MARKERS: [&str; 5] = ["featuring", "feat.", "feat", "ft.", "ft"];

/// Split `text` at the first featured-artist marker: what comes before it, and the featured
/// names after it. A marker counts only as a whole word that starts the text or follows a
/// space or an opening bracket.
fn split_marker(text: &str) -> (String, Vec<String>) {
    // ASCII lowercasing keeps every byte offset, so indexes into `text` stay valid.
    let lower = text.to_ascii_lowercase();
    for (i, _) in lower.char_indices() {
        let starts_word = lower[..i]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || c == '(' || c == '[');
        if !starts_word {
            continue;
        }
        let Some(marker) = MARKERS.iter().find(|m| lower[i..].starts_with(**m)) else {
            continue;
        };
        let after = &lower[i + marker.len()..];
        if !after.chars().next().is_some_and(|c| c.is_whitespace()) {
            continue; // "feather", "ft"-prefixed words
        }
        let before =
            text[..i].trim_end_matches(|c: char| c.is_whitespace() || c == '(' || c == '[');
        let rest = text[i + marker.len()..]
            .trim()
            .trim_end_matches([')', ']'])
            .trim();
        let names = rest
            .replace(" & ", ",")
            .replace(" and ", ",")
            .split(',')
            .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|n| !n.is_empty())
            .collect();
        return (before.trim().to_string(), names);
    }
    (text.trim().to_string(), Vec::new())
}

/// The credits of a track: the artist field without its `feat.` part is the primary artist, and
/// featured names come from the artist field and the title (each once, ignoring case).
pub fn parse_credits(artist: Option<&str>, title: &str) -> Credits {
    let (primary, mut featured) = match artist {
        Some(a) => {
            let (p, f) = split_marker(a);
            ((!p.is_empty()).then_some(p), f)
        }
        None => (None, Vec::new()),
    };
    featured.extend(split_marker(title).1);
    let mut seen = Vec::new();
    featured.retain(|n| {
        let key = crate::entity_norm(n);
        let new =
            !seen.contains(&key) && primary.as_deref().map(crate::entity_norm) != Some(key.clone());
        seen.push(key);
        new
    });
    Credits { primary, featured }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits(artist: &str, title: &str) -> (Option<String>, Vec<String>) {
        let c = parse_credits(Some(artist), title);
        (c.primary, c.featured)
    }

    #[test]
    fn the_artist_field_is_split_at_a_marker() {
        for text in [
            "Aventura feat. Romeo Santos",
            "Aventura ft. Romeo Santos",
            "Aventura Feat Romeo Santos",
            "Aventura featuring Romeo Santos",
            "Aventura (feat. Romeo Santos)",
            "Aventura [ft. Romeo Santos]",
        ] {
            assert_eq!(
                credits(text, "Song"),
                (Some("Aventura".into()), vec!["Romeo Santos".into()]),
                "{text}"
            );
        }
    }

    #[test]
    fn several_featured_names_and_the_title_are_read_too() {
        assert_eq!(
            credits("Kaysha feat. Sally, Ben & Joe", "Song (ft. Ann)"),
            (
                Some("Kaysha".into()),
                vec!["Sally".into(), "Ben".into(), "Joe".into(), "Ann".into()]
            )
        );
        // each name once, and never the primary artist itself
        assert_eq!(
            credits("A feat. B", "Song (feat. b and A)").1,
            vec!["B".to_string()]
        );
    }

    #[test]
    fn duos_and_lookalike_words_are_left_alone() {
        assert_eq!(
            credits("Simon & Garfunkel", "Song"),
            (Some("Simon & Garfunkel".into()), vec![])
        );
        assert_eq!(
            credits("Feather Band", "Soft Feat"),
            (Some("Feather Band".into()), vec![])
        );
        assert_eq!(
            credits("Soft", "Raft of Feats"),
            (Some("Soft".into()), vec![])
        );
        assert_eq!(parse_credits(None, "Song").primary, None);
    }

    #[test]
    fn a_marker_with_no_primary_leaves_none() {
        assert_eq!(
            parse_credits(Some("feat. Someone"), "Song"),
            Credits {
                primary: None,
                featured: vec!["Someone".into()]
            }
        );
    }
}
