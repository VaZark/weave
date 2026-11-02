//! Asking whether artists with similar names are one person. Nothing here merges on its own:
//! a pair is merged only when the user answers so, and different people (the same name can be
//! shared) are remembered so they are not asked about again.

use std::collections::HashSet;
use std::io::{BufRead, Write};

use weave::Library;
use weave::core::{Result, SimilarArtists};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub merged: usize,
    pub kept_separate: usize,
    pub skipped: usize,
}

/// Pairs worth asking about: all of them, or only those with an artist that was not there
/// `before` (what an import just brought in).
pub fn pending(lib: &Library, before: Option<&HashSet<String>>) -> Result<Vec<SimilarArtists>> {
    Ok(lib
        .similar_artists()?
        .into_iter()
        .filter(|p| before.is_none_or(|known| !known.contains(&p.a.id) || !known.contains(&p.b.id)))
        .collect())
}

/// Ask about each pair, applying the answers. Stops at `q` or when the input ends.
pub fn review(
    lib: &mut Library,
    mut pairs: Vec<SimilarArtists>,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<Outcome> {
    let mut outcome = Outcome::default();
    let io = |e: std::io::Error| weave::core::Error::InvalidInput(e.to_string());
    while !pairs.is_empty() {
        let SimilarArtists { a, b } = pairs.remove(0);
        writeln!(
            out,
            "Same person? \"{}\" ({}, {} tracks) and \"{}\" ({}, {} tracks)",
            a.name, a.id, a.tracks, b.name, b.id, b.tracks
        )
        .map_err(io)?;
        writeln!(
            out,
            "  [m] merge \"{}\" into \"{}\"    [r] merge \"{}\" into \"{}\"",
            b.name, a.name, a.name, b.name
        )
        .map_err(io)?;
        write!(
            out,
            "  [k] different people, don't ask again    [s] skip for now    [q] stop asking > "
        )
        .map_err(io)?;
        out.flush().map_err(io)?;
        let mut line = String::new();
        if input.read_line(&mut line).map_err(io)? == 0 {
            writeln!(out).map_err(io)?;
            break;
        }
        let gone = match line.trim().to_lowercase().as_str() {
            "m" => Some((&b, &a)),
            "r" => Some((&a, &b)),
            "k" => {
                lib.separate_artists(&a.id, &b.id)?;
                outcome.kept_separate += 1;
                continue;
            }
            "q" => break,
            _ => {
                outcome.skipped += 1;
                continue;
            }
        };
        if let Some((from, into)) = gone {
            let stats = lib.merge_artists(&from.id, &into.id)?;
            writeln!(
                out,
                "  merged \"{}\" into \"{}\" ({} tracks, {} albums moved)",
                from.name, into.name, stats.tracks, stats.albums
            )
            .map_err(io)?;
            outcome.merged += 1;
            // The artist that no longer exists cannot be asked about again.
            pairs.retain(|p| p.a.id != from.id && p.b.id != from.id);
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use weave::core::{FilterSpec, Metadata, ScannedTrack};

    fn library() -> Library {
        let mut lib = Library::open_in_memory().unwrap();
        let t = |p: &str, artist: &str| ScannedTrack {
            path: p.into(),
            metadata: Metadata {
                title: p.into(),
                artist: Some(artist.into()),
                ..Default::default()
            },
        };
        lib.import_scanned(&[
            t("/1", "Daft Punk"),
            t("/2", "Daft Punk"),
            t("/3", "Daft-Punk"),
            t("/4", "Sting"),
            t("/5", "STING."),
        ])
        .unwrap();
        lib
    }

    fn run(lib: &mut Library, answers: &str) -> (Outcome, String) {
        let pairs = pending(lib, None).unwrap();
        let mut out = Vec::new();
        let outcome = review(lib, pairs, &mut Cursor::new(answers.to_string()), &mut out).unwrap();
        (outcome, String::from_utf8(out).unwrap())
    }

    fn names(lib: &Library) -> Vec<String> {
        lib.artists(&FilterSpec::default())
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    #[test]
    fn merging_keeping_and_skipping_are_the_users_call() {
        let mut lib = library();
        // pairs come sorted by name: Daft Punk / Daft-Punk, then STING. / Sting
        let (o, text) = run(&mut lib, "m\nk\n");
        assert_eq!((o.merged, o.kept_separate, o.skipped), (1, 1, 0), "{text}");
        assert!(
            text.contains("merged \"Daft-Punk\" into \"Daft Punk\""),
            "{text}"
        );
        assert_eq!(names(&lib), ["Daft Punk", "Sting", "STING."]);
        // the kept pair is not asked again; nothing is left to ask
        assert!(pending(&lib, None).unwrap().is_empty());
    }

    #[test]
    fn skipping_leaves_the_question_open_and_the_input_ending_stops() {
        let mut lib = library();
        let (o, _) = run(&mut lib, "\ns\n");
        assert_eq!((o.merged, o.kept_separate, o.skipped), (0, 0, 2));
        assert_eq!(pending(&lib, None).unwrap().len(), 2);
        let (o, _) = run(&mut lib, "");
        assert_eq!(o, Outcome::default(), "end of input stops quietly");
        let (o, _) = run(&mut lib, "q\n");
        assert_eq!(o, Outcome::default());
        assert_eq!(names(&lib).len(), 4, "nothing was merged");
    }

    #[test]
    fn the_reverse_answer_merges_the_other_way() {
        let mut lib = library();
        run(&mut lib, "r\nq\n");
        assert_eq!(names(&lib), ["Daft-Punk", "Sting", "STING."]);
    }

    #[test]
    fn an_import_only_asks_about_what_it_brought() {
        let mut lib = library();
        let before: HashSet<String> = lib
            .artists(&FilterSpec::default())
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert!(pending(&lib, Some(&before)).unwrap().is_empty());
        lib.import_scanned(&[ScannedTrack {
            path: "/6".into(),
            metadata: Metadata {
                title: "six".into(),
                artist: Some("The Sting".into()),
                ..Default::default()
            },
        }])
        .unwrap();
        let asked = pending(&lib, Some(&before)).unwrap();
        assert!(
            !asked.is_empty()
                && asked
                    .iter()
                    .all(|p| p.a.name.contains("Sting") || p.b.name.contains("Sting"))
        );
    }
}
