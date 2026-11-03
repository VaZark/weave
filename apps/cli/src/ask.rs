//! Questions put to the user before something they cannot undo.

use std::io::{BufRead, Write};

use weave_player_lib::core::{Dependent, Playlist, PlaylistKind};

/// Show `message`, then ask the user to type `name` exactly (case included) to go ahead, the way
/// deleting a repository is confirmed. Anything else, and the input ending, is no.
pub fn confirm_by_name(
    message: &str,
    name: &str,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> std::io::Result<bool> {
    writeln!(out, "{message}")?;
    write!(
        out,
        "Type the name of the playlist (\"{name}\") to confirm: "
    )?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        writeln!(out)?;
        return Ok(false);
    }
    Ok(line.trim() == name)
}

/// What deleting a playlist would do, for the user to confirm: the playlist itself and any
/// playlists that draw on it.
pub fn delete_message(playlist: &Playlist, tracks: usize, dependents: &[Dependent]) -> String {
    let kind = match playlist.kind {
        PlaylistKind::Manual => format!("manual, {tracks} tracks"),
        PlaylistKind::Dynamic => "dynamic, a saved filter".to_string(),
    };
    let mut message = format!(
        "Delete playlist \"{}\" ({kind}) and its tags?",
        playlist.name
    );
    if !dependents.is_empty() {
        let list: Vec<String> = dependents
            .iter()
            .map(|d| {
                if d.only_source {
                    format!("{} (it would then use the whole library)", d.playlist.name)
                } else {
                    d.playlist.name.clone()
                }
            })
            .collect();
        message += &format!(
            "\nIt is a source of {} playlist(s), which lose it: {}.",
            dependents.len(),
            list.join(", ")
        );
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use weave_player_lib::core::{EntityRef, FilterSpec};

    fn ask(answer: &str) -> (bool, String) {
        let mut out = Vec::new();
        let yes = confirm_by_name(
            "Sure?",
            "Road trip",
            &mut Cursor::new(answer.to_string()),
            &mut out,
        )
        .unwrap();
        (yes, String::from_utf8(out).unwrap())
    }

    #[test]
    fn only_the_exact_name_confirms() {
        assert!(ask("Road trip\n").0);
        assert!(ask("  Road trip  \n").0, "surrounding spaces do not count");
        for wrong in ["y\n", "yes\n", "road trip\n", "road-trip\n", "Road\n", "\n"] {
            assert!(!ask(wrong).0, "{wrong:?}");
        }
        let (yes, text) = ask("");
        assert!(!yes, "the input ending is no");
        assert!(
            text.starts_with("Sure?\nType the name of the playlist (\"Road trip\") to confirm: ")
        );
    }

    fn playlist(name: &str, kind: PlaylistKind) -> Playlist {
        Playlist {
            id: name.to_lowercase(),
            name: name.into(),
            description: None,
            kind,
            filter: FilterSpec::default(),
        }
    }

    #[test]
    fn the_message_says_what_goes_and_who_loses_a_source() {
        let manual = playlist("Road trip", PlaylistKind::Manual);
        assert_eq!(
            delete_message(&manual, 8, &[]),
            "Delete playlist \"Road trip\" (manual, 8 tracks) and its tags?"
        );
        let dep = |name: &str, only| Dependent {
            playlist: EntityRef {
                id: name.to_lowercase(),
                name: name.into(),
            },
            only_source: only,
        };
        let dynamic = playlist("Mix", PlaylistKind::Dynamic);
        assert_eq!(
            delete_message(&dynamic, 0, &[dep("A", true), dep("B", false)]),
            "Delete playlist \"Mix\" (dynamic, a saved filter) and its tags?\n\
             It is a source of 2 playlist(s), which lose it: A (it would then use the whole library), B."
        );
    }
}
