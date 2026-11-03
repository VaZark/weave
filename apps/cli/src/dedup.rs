//! Asking whether artists, albums or tracks that look alike are the same. Nothing here merges
//! on its own: a pair is merged only when the user answers so, and things that are different
//! (people share names, and a live take is not the studio one) are remembered, so they are not
//! asked about again.

use std::io::{BufRead, Write};

use weave_player_lib::Library;
use weave_player_lib::core::{DedupMark, EntityKind, Result, SimilarPair};

/// The kinds that can have duplicates, in the order they are asked: artists first, since
/// merging two artists can leave two albums of the same name.
pub const KINDS: [EntityKind; 3] = [EntityKind::Artist, EntityKind::Album, EntityKind::Track];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub merged: usize,
    pub kept_separate: usize,
    pub skipped: usize,
}

impl Outcome {
    fn add(&mut self, other: Outcome) {
        self.merged += other.merged;
        self.kept_separate += other.kept_separate;
        self.skipped += other.skipped;
    }
}

/// Ask about each pair, applying the answers. Stops at `q` or when the input ends; the second
/// value says whether to carry on with the next kind.
pub fn review(
    lib: &mut Library,
    mut pairs: Vec<SimilarPair>,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<(Outcome, bool)> {
    let mut outcome = Outcome::default();
    let io = |e: std::io::Error| weave_player_lib::core::Error::InvalidInput(e.to_string());
    while !pairs.is_empty() {
        let SimilarPair { kind, a, b, reason } = pairs.remove(0);
        writeln!(out, "Same {kind}? ({reason})").map_err(io)?;
        writeln!(out, "    \"{}\"  {} [{}]", a.name, a.detail, a.id).map_err(io)?;
        writeln!(out, "    \"{}\"  {} [{}]", b.name, b.detail, b.id).map_err(io)?;
        write!(
            out,
            "  [m] merge the second into the first   [r] the first into the second\n  \
             [k] different, don't ask again   [s] skip for now   [q] stop asking > "
        )
        .map_err(io)?;
        out.flush().map_err(io)?;
        let mut line = String::new();
        if input.read_line(&mut line).map_err(io)? == 0 {
            writeln!(out).map_err(io)?;
            return Ok((outcome, false));
        }
        let (from, into) = match line.trim().to_lowercase().as_str() {
            "m" => (&b, &a),
            "r" => (&a, &b),
            "k" => {
                lib.separate(kind, &a.id, &b.id)?;
                outcome.kept_separate += 1;
                continue;
            }
            "q" => return Ok((outcome, false)),
            _ => {
                outcome.skipped += 1;
                continue;
            }
        };
        let stats = lib.merge(kind, &from.id, &into.id)?;
        writeln!(
            out,
            "  merged \"{}\" into \"{}\" ({} tracks, {} albums moved)",
            from.name, into.name, stats.tracks, stats.albums
        )
        .map_err(io)?;
        outcome.merged += 1;
        // What no longer exists cannot be asked about again.
        pairs.retain(|p| p.a.id != from.id && p.b.id != from.id);
    }
    Ok((outcome, true))
}

/// Go through the given kinds, asking afresh for each (an earlier answer can change a later
/// list). `since` limits it to what an import brought.
pub fn review_all(
    lib: &mut Library,
    kinds: &[EntityKind],
    since: Option<&DedupMark>,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<Outcome> {
    let mut total = Outcome::default();
    for kind in kinds {
        let pairs = lib.duplicates(*kind, since)?;
        let (outcome, carry_on) = review(lib, pairs, input, out)?;
        total.add(outcome);
        if !carry_on {
            break;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use weave_player_lib::core::{FilterSpec, Metadata, ScannedTrack};

    fn track(path: &str, title: &str, artist: &str, album: Option<&str>) -> ScannedTrack {
        ScannedTrack {
            path: path.into(),
            metadata: Metadata {
                title: title.into(),
                artist: Some(artist.into()),
                album: album.map(Into::into),
                duration_ms: Some(200_000),
                ..Default::default()
            },
        }
    }

    fn library() -> Library {
        let mut lib = Library::open_in_memory().unwrap();
        lib.import_scanned(&[
            track("/1", "one", "Daft Punk", None),
            track("/2", "two", "Daft Punk", None),
            track("/3", "three", "Daft-Punk", None),
            track("/4", "four", "Sting", None),
            track("/5", "five", "STING.", None),
        ])
        .unwrap();
        lib
    }

    fn run(lib: &mut Library, kinds: &[EntityKind], answers: &str) -> (Outcome, String) {
        let mut out = Vec::new();
        let outcome = review_all(
            lib,
            kinds,
            None,
            &mut Cursor::new(answers.to_string()),
            &mut out,
        )
        .unwrap();
        (outcome, String::from_utf8(out).unwrap())
    }

    fn artists(lib: &Library) -> Vec<String> {
        lib.artists(&FilterSpec::default())
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    #[test]
    fn merging_keeping_and_skipping_are_the_users_call() {
        let mut lib = library();
        // sorted by name: Daft Punk / Daft-Punk, then STING. / Sting
        let (o, text) = run(&mut lib, &[EntityKind::Artist], "m\nk\n");
        assert_eq!((o.merged, o.kept_separate, o.skipped), (1, 1, 0), "{text}");
        assert!(
            text.contains("Same artist?")
                && text.contains("merged \"Daft-Punk\" into \"Daft Punk\""),
            "{text}"
        );
        assert_eq!(artists(&lib), ["Daft Punk", "Sting", "STING."]);
        assert!(lib.duplicates(EntityKind::Artist, None).unwrap().is_empty());
    }

    #[test]
    fn skipping_leaves_the_question_open_and_the_input_ending_stops() {
        let mut lib = library();
        let (o, _) = run(&mut lib, &[EntityKind::Artist], "\ns\n");
        assert_eq!((o.merged, o.kept_separate, o.skipped), (0, 0, 2));
        assert_eq!(lib.duplicates(EntityKind::Artist, None).unwrap().len(), 2);
        assert_eq!(
            run(&mut lib, &[EntityKind::Artist], "").0,
            Outcome::default()
        );
        assert_eq!(
            run(&mut lib, &[EntityKind::Artist], "q\n").0,
            Outcome::default()
        );
        assert_eq!(artists(&lib).len(), 4, "nothing was merged");
    }

    #[test]
    fn the_reverse_answer_merges_the_other_way() {
        let mut lib = library();
        run(&mut lib, &[EntityKind::Artist], "r\nq\n");
        assert_eq!(artists(&lib), ["Daft-Punk", "Sting", "STING."]);
    }

    #[test]
    fn one_pass_covers_artists_then_albums_then_tracks() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.import_scanned(&[track("/a/1", "Una Noche", "Aventura", Some("Gold"))])
            .unwrap();
        lib.import_scanned(&[track("/b/1", "Una noche", "Aventura", Some("Gold"))])
            .unwrap();
        // merging the artists also folds their same-titled albums, so the album question
        // is gone; what is left is two copies of one track
        let (o, text) = run(
            &mut lib, &KINDS, "m
m
",
        );
        assert_eq!(o.merged, 2, "{text}");
        assert!(
            text.contains("Same artist?") && text.contains("Same track?"),
            "{text}"
        );
        assert!(!text.contains("Same album?"), "{text}");
        assert_eq!(artists(&lib), ["Aventura"]);
        let left = lib
            .search(&FilterSpec::default(), weave_player_lib::core::Page::ALL)
            .unwrap();
        assert_eq!(left.total, 1);
        // an album pair that is not settled by an artist merge is asked about
        let mut lib = Library::open_in_memory().unwrap();
        lib.import_scanned(&[track("/a/1", "One", "Aventura", Some("Gold"))])
            .unwrap();
        lib.import_scanned(&[track("/b/1", "Two", "Aventura", Some("GOLD!"))])
            .unwrap();
        let (o, text) = run(
            &mut lib,
            &[EntityKind::Album],
            "m
",
        );
        assert_eq!(o.merged, 1, "{text}");
        assert!(text.contains("Same album?"), "{text}");
    }

    #[test]
    fn an_import_only_asks_about_what_it_brought() {
        let mut lib = library();
        let mark = lib.dedup_mark().unwrap();
        assert!(
            lib.duplicates(EntityKind::Artist, Some(&mark))
                .unwrap()
                .is_empty()
        );
        lib.import_scanned(&[track("/6", "six", "The Sting", None)])
            .unwrap();
        let asked = lib.duplicates(EntityKind::Artist, Some(&mark)).unwrap();
        assert!(
            !asked.is_empty()
                && asked
                    .iter()
                    .all(|p| p.a.name.contains("Sting") || p.b.name.contains("Sting"))
        );
    }
}
