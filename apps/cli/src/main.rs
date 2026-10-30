/// `println!` that ends the process quietly when stdout is closed (`weave-player list | head`).
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write;
        if writeln!(std::io::stdout(), $($arg)*).is_err() {
            std::process::exit(0);
        }
    }};
}

#[cfg(debug_assertions)]
mod dev;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::builder::BoolishValueParser;
use clap::{Args, Parser, Subcommand};
use weave_core::{
    EndOfQueue, Error, FilterSpec, Page, RepeatMode, Sort, SortField, parse_duration,
};
use weave_engine::Library;

use output::{
    out, print_columns, print_json, print_page, print_playlist, print_playlists, print_queue,
    print_session,
};

/// Exit codes: 0 ok, 1 general failure, 2 usage, 3 not found, 4 invalid input/filter/state.
#[derive(Parser)]
#[command(
    name = "weave-player",
    version,
    about = "Local-first music library with tags, playlists and a persistent play queue"
)]
struct Cli {
    /// Library database path.
    #[arg(long, global = true, env = "WEAVE_DB")]
    db: Option<PathBuf>,
    /// Emit structured JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Add an ID column (the slug used to address tracks and playlists) to tables.
    #[arg(long, global = true)]
    show_ids: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Index audio files without modifying them.
    Import { path: PathBuf },
    /// List tracks.
    List {
        #[command(flatten)]
        result: ResultArgs,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Search by tags, BPM, metadata, favorites and ratings.
    Search {
        #[command(flatten)]
        filter: FilterArgs,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Add tags to a track, or to the current track when no ID is given.
    Add {
        track_id: Option<String>,
        #[arg(long = "tag", required = true, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Remove tags from a track.
    Remove {
        track_id: String,
        #[arg(long = "tag", required = true, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Show a track's tags.
    Tags { track_id: String },
    /// Mark tracks as favorites.
    Favorite {
        #[arg(required = true)]
        track_ids: Vec<String>,
    },
    /// Remove tracks from the favorites.
    Unfavorite {
        #[arg(required = true)]
        track_ids: Vec<String>,
    },
    /// Rate a track from 1 to 5 stars; 0 clears the rating.
    Rate {
        track_id: String,
        #[arg(value_parser = clap::value_parser!(u8).range(0..=5))]
        stars: u8,
    },
    /// Manage playlists.
    #[command(subcommand)]
    Playlist(PlaylistCmd),
    /// Show and edit the play queue.
    #[command(subcommand)]
    Queue(QueueCmd),
    /// Replace the queue with one track and start it. Session state only: audio output arrives in 0.a4.
    Play { track_id: String },
    /// Pause the current track.
    Pause,
    /// Resume a paused track, or restart the current one after a stop.
    Resume,
    /// Stop playback; the queue and position are kept.
    Stop,
    /// Skip to the next track (starts an idle queue).
    Next,
    /// Go back to the previous track.
    Prev,
    /// Show the playback session: status, current track, queue position and modes.
    Now,
    /// Turn shuffle on or off for the queue.
    Shuffle {
        #[arg(value_parser = BoolishValueParser::new(), action = clap::ArgAction::Set, value_name = "on|off")]
        on: bool,
    },
    /// Repeat nothing, the current song, or the whole queue (playlist).
    Repeat {
        #[arg(value_name = "off|song|playlist")]
        mode: RepeatMode,
    },
    /// Stop playback after a delay (30m, 1h30m, 45s; a bare number is minutes), or `off` to cancel.
    Sleep {
        #[arg(value_name = "DURATION|off")]
        after: String,
    },
    /// Inspect backends.
    #[command(subcommand)]
    Backend(BackendCmd),
    /// Developer tools (debug builds only).
    #[cfg(debug_assertions)]
    #[command(subcommand)]
    Dev(DevCmd),
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
enum DevCmd {
    /// Fill the library with fake tracks, tags, ratings and playlists (idempotent; no files are read).
    Seed {
        /// Number of tracks to generate.
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=dev::MAX_COUNT as i64))]
        count: u32,
    },
    /// Simulate the current track finishing by itself (what a playback backend will report).
    FinishTrack,
}

#[derive(Subcommand)]
enum PlaylistCmd {
    /// Save a dynamic playlist (filters, sort and limit, not membership), or an empty manual one.
    Create {
        name: String,
        /// What the playlist is for.
        #[arg(long)]
        description: Option<String>,
        /// Create an ordered list you fill with `playlist add` instead of a saved filter.
        #[arg(long)]
        manual: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Show a playlist's tracks (evaluates the saved filters of a dynamic one).
    Show {
        name: String,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Change the specified filters of a dynamic playlist; retain the others.
    Update {
        name: String,
        /// New description; pass "" to clear it.
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Append tracks to a manual playlist.
    Add {
        name: String,
        #[arg(required = true)]
        track_ids: Vec<String>,
    },
    /// Remove tracks from a manual playlist.
    Remove {
        name: String,
        #[arg(required = true)]
        track_ids: Vec<String>,
    },
    /// Replace the queue with the playlist's tracks and start it (or add them with --append).
    Play {
        name: String,
        /// Add to the end of the queue instead of replacing it.
        #[arg(long)]
        append: bool,
    },
    /// List saved playlists.
    List,
}

#[derive(Subcommand)]
enum QueueCmd {
    /// Show the queue.
    Show,
    /// Add tracks (or a playlist's tracks) to the end of the queue.
    Add {
        #[arg(required_unless_present = "playlist")]
        track_ids: Vec<String>,
        /// Add this playlist's tracks.
        #[arg(long)]
        playlist: Option<String>,
        /// Insert after the current track instead of at the end.
        #[arg(long)]
        next: bool,
    },
    /// Remove tracks by position (1-based, as shown by `queue show`).
    Remove {
        #[arg(required = true, value_parser = clap::value_parser!(usize))]
        positions: Vec<usize>,
    },
    /// Move a track from one position to another.
    Move { from: usize, to: usize },
    /// Make the track at a position current and start it.
    Jump { position: usize },
    /// Empty the queue and stop.
    Clear,
    /// What happens when the queue runs out: stop, or continue with similar tracks
    /// (shared tags, BPM near the queue's average).
    End {
        #[arg(value_name = "stop|continue")]
        mode: EndOfQueue,
    },
}

#[derive(Subcommand)]
enum BackendCmd {
    List,
    Show { id: String },
}

#[derive(Args, Clone, Copy)]
struct PageArgs {
    /// Page number, starting at 1.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    page: u32,
    /// Tracks per page.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..))]
    per_page: u32,
    /// Print every match instead of one page.
    #[arg(long, conflicts_with_all = ["page", "per_page"])]
    all: bool,
}

impl PageArgs {
    fn page(self) -> Page {
        if self.all {
            Page::ALL
        } else {
            Page::numbered(self.page as usize, self.per_page as usize)
        }
    }
}

/// Ordering and a cap on the result set.
#[derive(Args, Clone, Copy, Default)]
struct ResultArgs {
    /// Order by title, artist, album, genre, year, bpm, duration, rating or added.
    /// Unknown values sort last.
    #[arg(long, value_name = "FIELD")]
    sort: Option<SortField>,
    /// Sort descending.
    #[arg(long, requires = "sort")]
    desc: bool,
    /// Keep at most this many matches (before paging).
    #[arg(long, value_parser = clap::value_parser!(usize))]
    limit: Option<usize>,
}

impl ResultArgs {
    fn spec(self) -> FilterSpec {
        FilterSpec {
            sort: self.sort.map(|field| Sort {
                field,
                descending: self.desc,
            }),
            limit: self.limit,
            ..Default::default()
        }
    }
}

#[derive(Args, Default)]
struct FilterArgs {
    /// Require every tag.
    #[arg(long, value_delimiter = ',')]
    tag_all: Vec<String>,
    /// Require at least one tag.
    #[arg(long, value_delimiter = ',')]
    tag_any: Vec<String>,
    /// Exclude tracks with any of these tags.
    #[arg(long, value_delimiter = ',')]
    tag_none: Vec<String>,
    #[arg(long)]
    bpm_min: Option<f64>,
    #[arg(long)]
    bpm_max: Option<f64>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    artist: Option<String>,
    #[arg(long)]
    album: Option<String>,
    #[arg(long)]
    genre: Option<String>,
    #[arg(long)]
    year_min: Option<i32>,
    #[arg(long)]
    year_max: Option<i32>,
    /// Only favorites.
    #[arg(long, conflicts_with = "not_favorite")]
    favorite: bool,
    /// Only tracks that are not favorites.
    #[arg(long)]
    not_favorite: bool,
    /// Minimum rating (1-5); unrated tracks never match.
    #[arg(long)]
    rating_min: Option<u8>,
    /// Maximum rating (1-5).
    #[arg(long)]
    rating_max: Option<u8>,
    #[command(flatten)]
    result: ResultArgs,
}

impl From<FilterArgs> for FilterSpec {
    fn from(a: FilterArgs) -> Self {
        FilterSpec {
            tag_all: a.tag_all,
            tag_any: a.tag_any,
            tag_none: a.tag_none,
            bpm_min: a.bpm_min,
            bpm_max: a.bpm_max,
            title: a.title,
            artist: a.artist,
            album: a.album,
            genre: a.genre,
            year_min: a.year_min,
            year_max: a.year_max,
            favorite: match (a.favorite, a.not_favorite) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            },
            rating_min: a.rating_min,
            rating_max: a.rating_max,
            ..a.result.spec()
        }
        .normalized()
    }
}

enum Failure {
    Engine(Error),
    Other(String),
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure::Engine(e)
    }
}

fn default_db() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("weave").join("library.db")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            let (code, msg) = match f {
                Failure::Engine(e) => (
                    match e {
                        Error::NotFound(_) => 3,
                        Error::InvalidFilter(_)
                        | Error::InvalidInput(_)
                        | Error::AlreadyExists(_) => 4,
                        _ => 1,
                    },
                    e.to_string(),
                ),
                Failure::Other(m) => (1, m),
            };
            eprintln!("weave-player: {msg}");
            ExitCode::from(code)
        }
    }
}

const NO_AUDIO_YET: &str =
    "note: no audio output yet (planned for 0.a4); this updates the playback session only";

fn run(cli: Cli) -> Result<(), Failure> {
    let json = cli.json;
    let show_ids = cli.show_ids;
    let mut lib = Library::open(&cli.db.unwrap_or_else(default_db))?;
    let session = |v: &weave_engine::SessionView| print_session(json, show_ids, v);

    match cli.command {
        Cmd::Import { path } => {
            let s = lib.import(&path)?;
            out(
                json,
                &serde_json::json!({ "added": s.added, "skipped": s.skipped }),
                || outln!("imported {} new, {} already indexed", s.added, s.skipped),
            )?;
        }
        Cmd::List { result, page } => print_page(
            json,
            show_ids,
            page,
            &lib.search(&result.spec(), page.page())?,
        )?,
        Cmd::Search { filter, page } => print_page(
            json,
            show_ids,
            page,
            &lib.search(&filter.into(), page.page())?,
        )?,
        Cmd::Add { track_id, tags } => {
            let (id, label) = match track_id {
                Some(id) => (id.clone(), id),
                None => {
                    let t = lib.current_track()?;
                    (t.id.0.clone(), t.metadata.title)
                }
            };
            let n = lib.add_tags(&id, &tags)?;
            out(json, &serde_json::json!({ "added": n }), || {
                outln!("added {n} tag(s) to {label}")
            })?;
        }
        Cmd::Remove { track_id, tags } => {
            let n = lib.remove_tags(&track_id, &tags)?;
            out(json, &serde_json::json!({ "removed": n }), || {
                outln!("removed {n} tag(s)")
            })?;
        }
        Cmd::Tags { track_id } => {
            let tags = lib.tags(&track_id)?;
            out(json, &tags, || tags.iter().for_each(|t| outln!("{t}")))?;
        }
        Cmd::Favorite { track_ids } => {
            let n = lib.set_favorite(&track_ids, true)?;
            out(json, &serde_json::json!({ "changed": n }), || {
                outln!("{n} track(s) added to favorites")
            })?;
        }
        Cmd::Unfavorite { track_ids } => {
            let n = lib.set_favorite(&track_ids, false)?;
            out(json, &serde_json::json!({ "changed": n }), || {
                outln!("{n} track(s) removed from favorites")
            })?;
        }
        Cmd::Rate { track_id, stars } => {
            lib.set_rating(&track_id, (stars > 0).then_some(stars))?;
            out(json, &serde_json::json!({ "rating": stars }), || {
                if stars == 0 {
                    outln!("rating cleared")
                } else {
                    outln!("rated {stars}/5")
                }
            })?;
        }

        Cmd::Playlist(PlaylistCmd::Create {
            name,
            description,
            manual,
            filter,
        }) => {
            let p = if manual {
                if FilterSpec::from(filter) != FilterSpec::default() {
                    return Err(Error::InvalidInput(
                        "manual playlists have no filters; fill them with `playlist add`".into(),
                    )
                    .into());
                }
                lib.create_manual_playlist(&name, description.as_deref())?
            } else {
                lib.create_playlist(&name, description.as_deref(), filter.into())?
            };
            print_playlist(json, show_ids, &p)?;
        }
        Cmd::Playlist(PlaylistCmd::Update {
            name,
            description,
            filter,
        }) => {
            let p = lib.update_playlist(&name, description.as_deref(), filter.into())?;
            print_playlist(json, show_ids, &p)?;
        }
        Cmd::Playlist(PlaylistCmd::Show { name, page }) => {
            let (_, result) = lib.playlist_tracks(&name, page.page())?;
            print_page(json, show_ids, page, &result)?;
        }
        Cmd::Playlist(PlaylistCmd::Add { name, track_ids }) => {
            let n = lib.playlist_add(&name, &track_ids)?;
            out(json, &serde_json::json!({ "added": n }), || {
                outln!("added {n} track(s)")
            })?;
        }
        Cmd::Playlist(PlaylistCmd::Remove { name, track_ids }) => {
            let n = lib.playlist_remove(&name, &track_ids)?;
            out(json, &serde_json::json!({ "removed": n }), || {
                outln!("removed {n} track(s)")
            })?;
        }
        Cmd::Playlist(PlaylistCmd::Play { name, append }) => {
            let v = lib.play_playlist(&name, append)?;
            session(&v)?;
            if !json {
                eprintln!("{NO_AUDIO_YET}");
            }
        }
        Cmd::Playlist(PlaylistCmd::List) => print_playlists(json, show_ids, &lib.playlists()?)?,

        Cmd::Queue(QueueCmd::Show) => print_queue(json, show_ids, &lib.session()?)?,
        Cmd::Queue(QueueCmd::Add {
            track_ids,
            playlist,
            next,
        }) => {
            let n = lib.queue_add_all(&track_ids, playlist.as_deref(), next)?;
            out(json, &serde_json::json!({ "added": n }), || {
                outln!("queued {n} track(s)")
            })?;
        }
        Cmd::Queue(QueueCmd::Remove { positions }) => session(&lib.queue_remove(&positions)?)?,
        Cmd::Queue(QueueCmd::Move { from, to }) => {
            print_queue(json, show_ids, &lib.queue_move(from, to)?)?
        }
        Cmd::Queue(QueueCmd::Jump { position }) => session(&lib.queue_jump(position)?)?,
        Cmd::Queue(QueueCmd::Clear) => session(&lib.queue_clear()?)?,
        Cmd::Queue(QueueCmd::End { mode }) => session(&lib.set_end_of_queue(mode)?)?,

        Cmd::Play { track_id } => {
            session(&lib.play_track(&track_id)?)?;
            if !json {
                eprintln!("{NO_AUDIO_YET}");
            }
        }
        Cmd::Pause => session(&lib.pause()?)?,
        Cmd::Resume => session(&lib.resume()?)?,
        Cmd::Stop => session(&lib.stop()?)?,
        Cmd::Next => session(&lib.next()?)?,
        Cmd::Prev => session(&lib.previous()?)?,
        Cmd::Now => session(&lib.session()?)?,
        Cmd::Shuffle { on } => session(&lib.set_shuffle(on)?)?,
        Cmd::Repeat { mode } => session(&lib.set_repeat(mode)?)?,
        Cmd::Sleep { after } => {
            let after = if after.eq_ignore_ascii_case("off") {
                None
            } else {
                Some(parse_duration(&after).map_err(Error::InvalidInput)?)
            };
            session(&lib.set_sleep(after)?)?;
        }

        Cmd::Backend(BackendCmd::List) => {
            let bs = lib.backends();
            if json {
                print_json(
                    &bs.iter()
                        .map(|b| serde_json::json!({ "id": b.id, "capabilities": b.capabilities }))
                        .collect::<Vec<_>>(),
                )?;
            } else {
                bs.iter()
                    .for_each(|b| outln!("{}\t{} capabilities", b.id, b.capabilities.len()));
            }
        }
        Cmd::Backend(BackendCmd::Show { id }) => {
            let b = lib.backend(&id)?;
            if json {
                print_json(&serde_json::json!({ "id": b.id, "capabilities": b.capabilities }))?;
            } else {
                outln!("{}", b.id);
                let rows = b
                    .capabilities
                    .iter()
                    .map(|c| {
                        vec![
                            format!("{:?}/{}", c.group, c.name),
                            c.state.supported.to_string(),
                            c.state.authorized.to_string(),
                            c.state.available.to_string(),
                        ]
                    })
                    .collect();
                print_columns(
                    &["CAPABILITY", "SUPPORTED", "AUTHORIZED", "AVAILABLE"],
                    rows,
                    &[],
                    None,
                );
            }
        }

        #[cfg(debug_assertions)]
        Cmd::Dev(DevCmd::Seed { count }) => {
            let s = dev::seed(&mut lib, count as usize)?;
            out(
                json,
                &serde_json::json!({
                    "added": s.added, "skipped": s.skipped, "tag_links_added": s.tagged,
                    "playlists_created": s.playlists,
                }),
                || {
                    outln!(
                        "seeded {} new track(s) ({} already present), {} tag link(s), {} playlist(s)",
                        s.added,
                        s.skipped,
                        s.tagged,
                        s.playlists
                    )
                },
            )?;
        }
        #[cfg(debug_assertions)]
        Cmd::Dev(DevCmd::FinishTrack) => session(&lib.track_ended()?)?,
    }
    Ok(())
}
