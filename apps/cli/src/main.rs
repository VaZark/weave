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
mod host;
mod lyrics;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::builder::BoolishValueParser;
use clap::{Args, Parser, Subcommand};
use weave::core::{
    ChildPolicy, EndOfQueue, Error, FilterSpec, Page, RepeatMode, SleepSetting, Sort, SortField,
    TagClause, TagOp, TagRange, parse_duration, parse_span,
};
use weave::{Library, Player, SessionView};

use output::{
    out, print_columns, print_page, print_playlist, print_playlist_tracks, print_playlists,
    print_queue, print_session,
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
    /// Never start the playback host automatically.
    #[arg(
        long,
        global = true,
        env = "WEAVE_NO_HOST",
        value_parser = BoolishValueParser::new()
    )]
    no_host: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum TagCmd {
    /// Create a tag, like `genre:bachata` or `practice`, optionally under a parent.
    Create {
        name: String,
        #[arg(long)]
        parent: Option<String>,
    },
    /// List tags with their parents and track counts.
    List,
    /// Set a tag's parent, or clear it. Assignments do not change, only what the parent matches.
    Parent {
        name: String,
        #[arg(conflicts_with = "clear", required_unless_present = "clear")]
        parent: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Delete a tag and its assignments; say what happens to its children.
    Delete {
        name: String,
        /// Children become top-level tags.
        #[arg(long, group = "children")]
        detach_children: bool,
        /// Children and their descendants are deleted too.
        #[arg(long, group = "children")]
        delete_children: bool,
    },
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
    /// Manage the tag vocabulary and hierarchy.
    #[command(subcommand)]
    Tag(TagCmd),
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
    /// Start playing a single track, a playlist, or a random order of what some filters match.
    /// Whatever starts it, it is the same now-playing queue: tracks can be added to it afterwards.
    #[command(subcommand)]
    Play(PlayCmd),
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
    /// Stop playback after a delay (45s, 30m, 1h30m; a unit is required), or `off` to cancel.
    Sleep {
        #[arg(value_name = "DURATION|off")]
        after: String,
        /// When the delay runs out during a track, let that track finish and stop after it
        /// (instead of cutting it off). The stop is scheduled when the track starts.
        #[arg(long)]
        stop_after: bool,
    },
    /// Run the playback host in the foreground: it plays the session, applies the sleep timer on
    /// time and logs what happens. Commands that start playback launch it for you.
    Host {
        /// Follow the session without producing sound.
        #[arg(long)]
        no_audio: bool,
        /// Do not register with the OS media controls (media keys, now-playing widgets).
        #[arg(long)]
        no_media_controls: bool,
        /// Exit after this long instead of running until `quit` or Ctrl-C.
        #[arg(long = "for", value_name = "DURATION")]
        run_for: Option<String>,
    },
    /// Stop the playback host.
    Quit,
    /// Jump within the current track: `1:30`, `90s`, `1m30s`, or relative with `+10s` / `-10s`.
    Seek {
        #[arg(value_name = "POSITION", allow_hyphen_values = true)]
        target: String,
    },
    /// Set the volume (0-100, `%` optional); without a value, show the session.
    Volume {
        #[arg(value_name = "0-100")]
        level: Option<String>,
    },
    /// Show a track's lyrics (the current track by default); `--follow` prints each line as it
    /// begins while the session plays.
    Lyrics {
        track_id: Option<String>,
        /// Keep running and print every synchronized line when it comes up.
        #[arg(long, conflicts_with = "track_id")]
        follow: bool,
        /// With --follow, exit after this long.
        #[arg(long = "for", value_name = "DURATION", requires = "follow")]
        run_for: Option<String>,
    },
    /// Show or change this instance: the name it goes by on the network and whether it may play.
    #[command(subcommand)]
    Instance(InstanceCmd),
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

// Parsed once per run, so the size difference between variants does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum PlayCmd {
    /// Play one track, in list order (shuffle off).
    Track { track_id: String },
    /// Play a playlist in its own order, or shuffled from a random track with --shuffle.
    Playlist {
        name: String,
        /// Turn shuffle on. Without it the playlist plays in list order (shuffle is turned off).
        #[arg(long)]
        shuffle: bool,
    },
    /// Play everything matching the filters (the whole library if none) in random order, from a
    /// random track, with shuffle on.
    Random {
        #[command(flatten)]
        filter: FilterArgs,
    },
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
    /// Move a track of a manual playlist to another place in its play order (1-based positions).
    Move {
        name: String,
        from: usize,
        to: usize,
    },
    /// Show a playlist's tracks with their play order (evaluates the saved filters of a dynamic one).
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
    /// List saved playlists.
    List,
}

#[derive(Subcommand)]
enum QueueCmd {
    /// Show the queue.
    Show,
    /// Add tracks, and/or the current tracks of a playlist (manual or dynamic), to the queue. A
    /// playlist is only a way to pick tracks: they are added as plain tracks.
    Add {
        #[arg(required_unless_present = "playlist")]
        track_ids: Vec<String>,
        /// Add this playlist's current tracks, in the playlist's order.
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
enum InstanceCmd {
    /// Name and capabilities of this instance.
    Show,
    /// Rename this instance (stored as a slug).
    Name { name: String },
    /// Allow this instance to play audio (`on`), or make it remote-only (`off`): it keeps its
    /// library and queue and can be controlled, but never makes sound.
    Playback {
        #[arg(value_parser = BoolishValueParser::new(), action = clap::ArgAction::Set, value_name = "on|off")]
        allowed: bool,
    },
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
    /// Everything on one page, for commands that show a whole list.
    fn everything() -> Self {
        PageArgs {
            page: 1,
            per_page: 50,
            all: true,
        }
    }

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
    /// Require every tag: `bachata,90s`, or by key `genre=bachata,sensual`. Repeatable.
    /// Matches child tags too; `genre==bachata` (or `=genre:bachata`) matches that tag only.
    #[arg(long, value_name = "[KEYS=]TAGS", value_parser = parse_tag_cond)]
    tag_all: Vec<TagCond>,
    /// Require at least one tag: `bachata,zouk`, `genre=bachata,sals`, `genre,style=salsa`.
    #[arg(long, value_name = "[KEYS=]TAGS", value_parser = parse_tag_cond)]
    tag_any: Vec<TagCond>,
    /// Exclude tracks with any of these tags: `live`, `period=80s,70s`.
    #[arg(long, value_name = "[KEYS=]TAGS", value_parser = parse_tag_cond)]
    tag_exclude: Vec<TagCond>,
    /// Numeric range on a keyed tag, ends optional: `bpm=90..120`, `bpm=100..`, `year=..1999`.
    #[arg(long, value_name = "KEY=MIN..MAX", value_parser = parse_tag_range)]
    tag_range: Vec<TagRange>,
    /// Case-insensitive part of the title. Genre, year and BPM are tags:
    /// `--tag-any genre=bachata`, `--tag-range year=1990..1999`.
    #[arg(long)]
    title: Option<String>,
    /// Case-insensitive part of the artist.
    #[arg(long)]
    artist: Option<String>,
    /// Case-insensitive part of the album.
    #[arg(long)]
    album: Option<String>,
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

/// One `--tag-*` value. Bare (`bachata,90s`): plain tag names. Keyed (`genre=bachata,sals`):
/// the tags `key:value`, for each of the comma-separated keys.
#[derive(Clone, Debug)]
struct TagCond {
    keys: Vec<String>,
    values: Vec<String>,
    exact: bool,
}

fn parse_tag_cond(text: &str) -> Result<TagCond, String> {
    let list = |s: &str| -> Vec<String> {
        s.split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
            .collect()
    };
    let (keys, values, exact) = match text.split_once('=').filter(|(k, _)| !k.trim().is_empty()) {
        Some((k, v)) => {
            let (v, exact) = v.strip_prefix('=').map_or((v, false), |v| (v, true));
            (list(k), list(v), exact)
        }
        None => (Vec::new(), list(text), false),
    };
    if values.is_empty() {
        return Err("expected tags, like `bachata,zouk` or `genre=bachata,sals`".into());
    }
    Ok(TagCond {
        keys,
        values,
        exact,
    })
}

fn parse_tag_range(text: &str) -> Result<TagRange, String> {
    let bad = || "expected KEY=MIN..MAX, like `bpm=90..120`, `bpm=100..` or `year=..1999`";
    let (key, range) = text.split_once('=').ok_or_else(bad)?;
    let (min, max) = range.split_once("..").ok_or_else(bad)?;
    let bound = |s: &str| -> Result<Option<f64>, String> {
        match s.trim() {
            "" => Ok(None),
            n => n
                .parse()
                .map(Some)
                .map_err(|_| format!("\"{n}\" is not a number")),
        }
    };
    Ok(TagRange {
        key: key.trim().to_string(),
        min: bound(min)?,
        max: bound(max)?,
    })
}

/// Bare conditions fill the flat tag lists; keyed ones become clauses.
fn split_conds(op: TagOp, conds: Vec<TagCond>) -> (Vec<String>, Vec<TagClause>) {
    let mut flat = Vec::new();
    let mut clauses = Vec::new();
    for c in conds {
        if c.keys.is_empty() {
            flat.extend(c.values);
        } else {
            clauses.push(TagClause {
                op,
                keys: c.keys,
                values: c.values,
                exact: c.exact,
            });
        }
    }
    (flat, clauses)
}

impl From<FilterArgs> for FilterSpec {
    fn from(a: FilterArgs) -> Self {
        let (tag_all, mut tag_clauses) = split_conds(TagOp::All, a.tag_all);
        let (tag_any, any) = split_conds(TagOp::Any, a.tag_any);
        let (tag_exclude, none) = split_conds(TagOp::Exclude, a.tag_exclude);
        tag_clauses.extend(any);
        tag_clauses.extend(none);
        FilterSpec {
            tag_all,
            tag_any,
            tag_exclude,
            tag_clauses,
            tag_range: a.tag_range,
            title: a.title,
            artist: a.artist,
            album: a.album,
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

#[derive(Debug)]
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

/// After a command that left the session playing: make sure a host is there to play it.
fn start_host_if_playing(db: &std::path::Path, cli: &HostPolicy, v: &SessionView) {
    use weave::core::PlayStatus;
    if cli.no_host || !cfg!(feature = "audio") || v.state.status != PlayStatus::Playing {
        return;
    }
    if cli.remote_only {
        if !cli.json {
            eprintln!(
                "this instance is remote-only: nothing plays here (`instance playback on` to change)"
            );
        }
        return;
    }
    match host::ensure_running(db) {
        Ok(Some(log)) if !cli.json => {
            eprintln!("started the playback host (log: {})", log.display())
        }
        Ok(_) => {}
        Err(e) => eprintln!("weave-player: could not start the playback host: {e}"),
    }
}

struct HostPolicy {
    no_host: bool,
    json: bool,
    /// This instance may not play audio, so no host is started.
    remote_only: bool,
}

fn run(cli: Cli) -> Result<(), Failure> {
    let json = cli.json;
    let show_ids = cli.show_ids;
    let db = cli.db.unwrap_or_else(default_db);
    let mut lib = Library::open(&db)?;
    let policy = HostPolicy {
        no_host: cli.no_host,
        json,
        remote_only: !lib.instance()?.playback,
    };
    let session = |v: &SessionView| print_session(json, show_ids, None, v);
    // Print the session, then start a host if it is playing and none runs.
    let playing = |v: &SessionView| -> Result<(), Failure> {
        print_session(json, show_ids, None, v)?;
        start_host_if_playing(&db, &policy, v);
        Ok(())
    };

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
        Cmd::Tag(TagCmd::Create { name, parent }) => {
            let tag = lib.create_tag(&name, parent.as_deref())?;
            out(json, &tag, || outln!("created tag {}", tag.name))?;
        }
        Cmd::Tag(TagCmd::List) => {
            let tags = lib.list_tags()?;
            out(json, &tags, || {
                let rows = tags
                    .iter()
                    .map(|t| {
                        vec![
                            t.name.clone(),
                            t.parent.clone().unwrap_or_else(|| "-".into()),
                            t.tracks.to_string(),
                        ]
                    })
                    .collect();
                print_columns(&["TAG", "PARENT", "TRACKS"], rows, &[], None);
            })?;
        }
        Cmd::Tag(TagCmd::Parent {
            name,
            parent,
            clear: _,
        }) => {
            let tag = lib.set_tag_parent(&name, parent.as_deref())?;
            out(json, &tag, || match &tag.parent {
                Some(p) => outln!("{} is now under {p}", tag.name),
                None => outln!("{} is now a top-level tag", tag.name),
            })?;
        }
        Cmd::Tag(TagCmd::Delete {
            name,
            detach_children,
            delete_children,
        }) => {
            let policy = match (detach_children, delete_children) {
                (true, _) => ChildPolicy::Detach,
                (_, true) => ChildPolicy::Delete,
                _ => {
                    return Err(Error::InvalidInput(
                        "say what happens to the children: --detach-children or --delete-children"
                            .into(),
                    )
                    .into());
                }
            };
            let n = lib.delete_tag(&name, policy)?;
            out(json, &serde_json::json!({ "deleted": n }), || {
                outln!("deleted {n} tag(s)")
            })?;
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
            print_playlist_tracks(json, show_ids, page, &result)?;
        }
        Cmd::Playlist(PlaylistCmd::Move { name, from, to }) => {
            lib.playlist_move(&name, from, to)?;
            let (_, result) = lib.playlist_tracks(&name, Page::ALL)?;
            print_playlist_tracks(json, show_ids, PageArgs::everything(), &result)?;
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
        Cmd::Playlist(PlaylistCmd::List) => print_playlists(json, show_ids, &lib.playlists()?)?,

        Cmd::Queue(QueueCmd::Show) => print_queue(json, show_ids, &lib.session()?)?,
        Cmd::Queue(QueueCmd::Add {
            track_ids,
            playlist,
            next,
        }) => {
            let n = lib.queue_add(&track_ids, playlist.as_deref(), next)?;
            out(json, &serde_json::json!({ "added": n }), || {
                outln!("queued {n} track(s)")
            })?;
        }
        Cmd::Queue(QueueCmd::Remove { positions }) => session(&lib.queue_remove(&positions)?)?,
        Cmd::Queue(QueueCmd::Move { from, to }) => {
            print_queue(json, show_ids, &lib.queue_move(from, to)?)?
        }
        Cmd::Queue(QueueCmd::Jump { position }) => playing(&lib.queue_jump(position)?)?,
        Cmd::Queue(QueueCmd::Clear) => session(&lib.queue_clear()?)?,
        Cmd::Queue(QueueCmd::End { mode }) => session(&lib.set_end_of_queue(mode)?)?,

        Cmd::Play(what) => playing(&match what {
            PlayCmd::Track { track_id } => lib.play_track(&track_id)?,
            PlayCmd::Playlist { name, shuffle } => lib.play_playlist(&name, shuffle)?,
            PlayCmd::Random { filter } => lib.play_random(filter.into())?,
        })?,
        Cmd::Pause => session(&lib.pause()?)?,
        Cmd::Resume => playing(&lib.resume()?)?,
        Cmd::Stop => session(&lib.stop()?)?,
        Cmd::Next => playing(&lib.next()?)?,
        Cmd::Prev => playing(&lib.previous()?)?,
        Cmd::Now => print_session(
            json,
            show_ids,
            Some(Player::is_running(&db)),
            &lib.session()?,
        )?,
        Cmd::Shuffle { on } => session(&lib.set_shuffle(on)?)?,
        Cmd::Repeat { mode } => session(&lib.set_repeat(mode)?)?,
        Cmd::Sleep { after, stop_after } => {
            let setting = if after.eq_ignore_ascii_case("off") {
                if stop_after {
                    return Err(Error::InvalidInput("--stop-after needs a duration".into()).into());
                }
                SleepSetting::Off
            } else {
                SleepSetting::In {
                    delay: parse_duration(&after).map_err(Error::InvalidInput)?,
                    stop_after,
                }
            };
            session(&lib.set_sleep(setting)?)?;
        }

        Cmd::Host {
            no_audio,
            no_media_controls,
            run_for,
        } => {
            let run_for = run_for
                .as_deref()
                .map(|s| parse_duration(s).map_err(Error::InvalidInput))
                .transpose()?;
            host::run_foreground(&db, json, no_audio, no_media_controls, run_for)?;
        }
        Cmd::Quit => {
            if !Player::is_running(&db) {
                outln!("no playback host is running");
            } else {
                lib.request_host_quit()?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while Player::is_running(&db) && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if Player::is_running(&db) {
                    return Err(Failure::Other(
                        "the playback host did not stop in time".into(),
                    ));
                }
                outln!("playback host stopped");
            }
        }
        Cmd::Seek { target } => {
            let position = lib.session()?.position;
            playing(&lib.seek(parse_position(&target, position)?)?)?;
        }
        Cmd::Volume { level } => match level {
            None => print_session(json, show_ids, None, &lib.session()?)?,
            Some(level) => session(&lib.set_volume(parse_volume(&level)?)?)?,
        },
        Cmd::Lyrics {
            track_id,
            follow,
            run_for,
        } => {
            if follow {
                let run_for = run_for
                    .as_deref()
                    .map(|s| parse_duration(s).map_err(Error::InvalidInput))
                    .transpose()?;
                lyrics::follow(&mut lib, json, run_for)?;
            } else {
                let current = track_id.is_none();
                let track = match &track_id {
                    Some(id) => lib.track(id)?,
                    None => lib.current_track()?,
                };
                let found = lib.lyrics(&track.id.0)?.ok_or_else(|| {
                    Error::NotFound(format!("lyrics for \"{}\"", track.metadata.title))
                })?;
                let position =
                    current.then(|| lib.session().map(|v| v.position.as_millis() as u64));
                lyrics::print(json, &track, &found, position.transpose()?)?;
            }
        }
        Cmd::Instance(cmd) => {
            let instance = match cmd {
                InstanceCmd::Show => lib.instance()?,
                InstanceCmd::Name { name } => lib.set_instance_name(&name)?,
                InstanceCmd::Playback { allowed } => lib.set_playback_allowed(allowed)?,
            };
            out(json, &instance, || {
                outln!("name: {}", instance.name);
                outln!(
                    "playback: {}",
                    if instance.playback {
                        "on"
                    } else {
                        "off (remote-only)"
                    }
                );
            })?;
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

/// `1:30`, `1:02:03`, `90s`, `1m30s`, or relative to `current` with a leading `+` / `-`.
fn parse_position(input: &str, current: std::time::Duration) -> Result<std::time::Duration, Error> {
    use std::time::Duration;
    let bad = |m: String| Error::InvalidInput(m);
    let absolute = |s: &str| -> Result<Duration, Error> {
        if s.contains(':') {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() > 3
                || parts
                    .iter()
                    .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(bad(format!("invalid position \"{s}\" (try 1:30 or 90s)")));
            }
            let secs = parts.iter().try_fold(0u64, |acc, p| {
                acc.checked_mul(60)
                    .and_then(|a| a.checked_add(p.parse().ok()?))
            });
            return secs
                .map(Duration::from_secs)
                .ok_or_else(|| bad(format!("position \"{s}\" is too large")));
        }
        parse_span(s).map_err(bad)
    };
    match input.trim().as_bytes().first() {
        Some(b'+') => Ok(current.saturating_add(absolute(input.trim()[1..].trim())?)),
        Some(b'-') => Ok(current.saturating_sub(absolute(input.trim()[1..].trim())?)),
        _ => absolute(input.trim()),
    }
}

/// `80` or `80%` to 0.0-1.0.
fn parse_volume(input: &str) -> Result<f32, Error> {
    let n: u8 = input
        .trim()
        .trim_end_matches('%')
        .parse()
        .map_err(|_| Error::InvalidInput(format!("invalid volume \"{input}\" (0-100)")))?;
    if n > 100 {
        return Err(Error::InvalidInput(
            "volume must be between 0 and 100".into(),
        ));
    }
    Ok(f32::from(n) / 100.0)
}

#[cfg(test)]
mod arg_tests {
    use std::time::Duration;

    use clap::Parser;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("weave-player").chain(args.iter().copied()))
    }

    #[test]
    fn play_takes_a_track_a_playlist_or_a_filtered_shuffle() {
        assert!(matches!(
            parse(&["play", "track", "some-track"]).unwrap().command,
            Cmd::Play(PlayCmd::Track { .. })
        ));
        let Cmd::Play(PlayCmd::Playlist { name, shuffle }) =
            parse(&["play", "playlist", "mix"]).unwrap().command
        else {
            panic!("not a playlist")
        };
        assert_eq!((name.as_str(), shuffle), ("mix", false));
        let Cmd::Play(PlayCmd::Playlist { shuffle, .. }) =
            parse(&["play", "playlist", "mix", "--shuffle"])
                .unwrap()
                .command
        else {
            panic!("not a playlist")
        };
        assert!(shuffle);

        let Cmd::Play(PlayCmd::Random { filter }) = parse(&[
            "play",
            "random",
            "--tag-all",
            "bachata,90s",
            "--tag-range",
            "bpm=90..",
            "--limit",
            "20",
        ])
        .unwrap()
        .command
        else {
            panic!("not a shuffle")
        };
        let spec = FilterSpec::from(filter);
        assert_eq!(
            (spec.tag_all, spec.tag_range[0].min, spec.limit),
            (vec!["bachata".into(), "90s".into()], Some(90.0), Some(20))
        );
        assert!(
            matches!(
                parse(&["play", "random"]).unwrap().command,
                Cmd::Play(PlayCmd::Random { .. })
            ),
            "filters are optional"
        );
        assert!(parse(&["play", "random", "--favorite", "--rating-min", "4"]).is_ok());
    }

    #[test]
    fn the_old_play_forms_are_gone() {
        assert!(
            parse(&["play", "a-track"]).is_err(),
            "a track needs `play track`"
        );
        assert!(
            parse(&["play", "shuffle"]).is_err(),
            "it is `play random` now"
        );
        assert!(parse(&["playlist", "play", "mix"]).is_err());
        assert!(
            parse(&["play"]).is_err(),
            "play alone asks for what to play"
        );
        assert!(
            parse(&["play", "playlist"]).is_err(),
            "a playlist is required"
        );
        assert!(
            parse(&["play", "track", "a", "b"]).is_err(),
            "one track only"
        );
        assert!(
            parse(&["play", "random", "--shuffle"]).is_err(),
            "random is always shuffled"
        );
    }

    #[test]
    fn playlist_move_takes_two_positions() {
        assert!(parse(&["playlist", "move", "mix", "3", "1"]).is_ok());
        assert!(parse(&["playlist", "move", "mix", "3"]).is_err());
        assert!(parse(&["playlist", "move", "mix", "x", "1"]).is_err());
    }

    #[test]
    fn queue_add_takes_tracks_and_or_a_playlist() {
        assert!(parse(&["queue", "add", "a", "b", "--next"]).is_ok());
        assert!(parse(&["queue", "add", "--playlist", "mix"]).is_ok());
        assert!(parse(&["queue", "add", "a", "--playlist", "mix", "--next"]).is_ok());
        assert!(
            parse(&["queue", "add"]).is_err(),
            "something to add is required"
        );
        assert!(parse(&["playlist", "play", "mix", "--append"]).is_err());
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn positions_absolute_and_relative() {
        let at = secs(100);
        assert_eq!(parse_position("1:30", at).unwrap(), secs(90));
        assert_eq!(parse_position("1:02:03", at).unwrap(), secs(3723));
        assert_eq!(parse_position("90s", at).unwrap(), secs(90));
        assert_eq!(parse_position("1m30s", at).unwrap(), secs(90));
        assert_eq!(parse_position("0:00", at).unwrap(), Duration::ZERO);
        assert_eq!(parse_position("0s", at).unwrap(), Duration::ZERO);
        assert_eq!(parse_position("+10s", at).unwrap(), secs(110));
        assert_eq!(parse_position("-30s", at).unwrap(), secs(70));
        assert_eq!(
            parse_position("-5m", at).unwrap(),
            Duration::ZERO,
            "never before the start"
        );
        assert_eq!(parse_position("+0:30", at).unwrap(), secs(130));
    }

    #[test]
    fn bad_positions_are_rejected_and_a_bare_number_needs_a_unit() {
        for bad in ["", "abc", "1:", ":30", "1:2:3:4", "12", "+", "1.5s"] {
            assert!(parse_position(bad, secs(0)).is_err(), "{bad}");
        }
    }

    #[test]
    fn keyed_tag_filters_become_clauses_and_bare_ones_stay_flat() {
        let Cmd::Search { filter, .. } = parse(&[
            "search",
            "--tag-any",
            "genre=bachata,sals",
            "--tag-all",
            "practice,90s",
            "--tag-all",
            "genre,style==salsa",
            "--tag-exclude",
            "period=80s",
            "--tag-range",
            "bpm=90..120",
        ])
        .unwrap()
        .command
        else {
            panic!("not a search")
        };
        let f = FilterSpec::from(filter);
        assert_eq!(f.tag_all, ["practice", "90s"]);
        assert_eq!(f.tag_clauses.len(), 3);
        let c = &f.tag_clauses[0];
        assert_eq!((c.op, c.exact, c.keys.len()), (TagOp::All, true, 2));
        assert_eq!(f.tag_clauses[1].values, ["bachata", "sals"]);
        assert_eq!(f.tag_clauses[2].op, TagOp::Exclude);
        assert_eq!(
            (f.tag_range[0].min, f.tag_range[0].max),
            (Some(90.0), Some(120.0))
        );
        assert_eq!(
            output::describe(&f),
            "--tag-all practice,90s --tag-all genre,style==salsa --tag-any genre=bachata,sals \
             --tag-exclude period=80s --tag-range bpm=90..120"
        );
        assert!(parse(&["search", "--tag-any", "genre="]).is_err());
        assert!(parse(&["search", "--tag-range", "bpm=fast"]).is_err());
        assert!(parse(&["search", "--tag-range", "bpm"]).is_err());
        let Cmd::Search { filter, .. } =
            parse(&["search", "--tag-exclude", "live"]).unwrap().command
        else {
            panic!()
        };
        assert_eq!(FilterSpec::from(filter).tag_exclude, ["live"]);
        assert!(
            parse(&["search", "--tag-none", "live"]).is_err(),
            "the old name is gone"
        );
    }

    #[test]
    fn the_command_line_is_well_formed_and_instance_commands_parse() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        let Cmd::Instance(InstanceCmd::Playback { allowed }) =
            parse(&["instance", "playback", "off"]).unwrap().command
        else {
            panic!("not an instance command")
        };
        assert!(!allowed);
        assert!(matches!(
            parse(&["instance"]).map(|c| c.command),
            Err(_) | Ok(Cmd::Instance(InstanceCmd::Show))
        ));
        assert!(parse(&["instance", "playback", "maybe"]).is_err());
    }

    #[test]
    fn volume_is_a_percentage() {
        assert_eq!(parse_volume("80").unwrap(), 0.8);
        assert_eq!(parse_volume("100%").unwrap(), 1.0);
        assert_eq!(parse_volume("0").unwrap(), 0.0);
        for bad in ["101", "-1", "abc", "", "5.5"] {
            assert!(parse_volume(bad).is_err(), "{bad}");
        }
    }
}
