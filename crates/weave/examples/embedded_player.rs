//! A tiny "player" that embeds weave: no CLI, no separate host process.
//!
//!     cargo run -p weave --example embedded_player --features audio,media-controls -- ~/Music
//!
//! It indexes the folder, starts the embedded player in this process, plays everything in random
//! order for a little while and prints what the player reports. Media keys work too (`media-controls`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use weave::{Library, Player, PlayerOptions};

fn main() -> weave::core::Result<()> {
    let music = std::env::args_os().nth(1).map(PathBuf::from);
    let db = std::env::temp_dir().join("weave-embedded-example.db");

    // The library handle: everything about tracks, playlists and the queue goes through it.
    let mut library = Library::open(&db)?;
    if let Some(music) = music {
        let stats = library.import(&music)?;
        println!("indexed {} new track(s)", stats.added);
    }

    // The player: plays whatever the session says, in this process, until it is dropped.
    let player = Player::start(&db, PlayerOptions::default())?;
    library.play_random(Default::default())?;

    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until && !player.is_finished() {
        if let Ok(event) = player.events().recv_timeout(Duration::from_millis(250)) {
            println!("{event}");
        }
    }
    Ok(())
}
