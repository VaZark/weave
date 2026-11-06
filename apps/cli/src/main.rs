/// `println!` that ends the process quietly when stdout is closed (`weave-player list | head`).
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write;
        if writeln!(std::io::stdout(), $($arg)*).is_err() {
            std::process::exit(0);
        }
    }};
}

mod ask;
mod clause_args;
mod dedup;
mod host;
mod lyrics;
mod output;
mod profile;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::builder::BoolishValueParser;
use clap::{Args, Parser, Subcommand};
use weave_player_lib::core::{
    ChildPolicy, EndOfQueue, EntityKind, Error, FilterSpec, Page, RepeatMode, SleepSetting, Sort,
    SortField, TagClause, TagOp, parse_duration, parse_span,
};
use weave_player_lib::{Library, Player, SessionView};

use output::{
    out, print_columns, print_page, print_playlist, print_playlist_tracks, print_playlists,
    print_queue, print_session,
};

/// Exit codes: 0 ok, 1 general failure, 2 usage, 3 not found, 4 invalid input/filter/state.
/// Which dev database `--mock-db` opens.
#[cfg(debug_assertions)]
#[derive(Clone, Copy, clap::ValueEnum)]
enum MockDb {
    Seeded,
    Empty,
}

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
    /// Use a separate dev database instead of the library (debug builds only): `seeded` (the
    /// default) is `dev.db`, filled with mock tracks, tags and playlists when it is empty;
    /// `empty` is a fresh `dev-empty.db`, wiped on every run. Never touches the real library;
    /// wins over --db and WEAVE_DB.
    #[cfg(debug_assertions)]
    #[arg(
        long,
        global = true,
        value_enum,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "seeded",
        value_name = "KIND"
    )]
    mock_db: Option<MockDb>,
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
enum DedupCmd {
    /// List things that look alike (all kinds, or only artist, album or track).
    List { kind: Option<EntityKind> },
    /// Go through them one by one: merge, keep separate, or skip.
    Ask { kind: Option<EntityKind> },
    /// Fold one into another (the one to keep): artists, albums or tracks.
    Merge {
        kind: EntityKind,
        from: String,
        into: String,
    },
    /// Say two are different, so they are not suggested again.
    Separate {
        kind: EntityKind,
        a: String,
        b: String,
    },
}

/// `artist <id>` shows a profile; the subcommands manage artists.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct ArtistArgs {
    /// The artist's ID (see `artist list`).
    id: Option<String>,
    #[command(subcommand)]
    cmd: Option<ArtistCmd>,
}

#[derive(Subcommand)]
enum ArtistCmd {
    /// Show an artist profile (for an artist whose ID is also a subcommand name).
    Show { id: String },
    /// List artists, optionally by the tags on the artists themselves.
    List {
        #[command(flatten)]
        tags: TagFilterArgs,
        #[arg(long, default_value = "")]
        query: String,
    },
    /// Add or remove tags on an artist.
    Tag(TagEdit),
    /// Split an artist in two for people who share a name: a new artist takes the given tracks
    /// and albums.
    Split {
        artist: String,
        /// Tracks that move to the new artist.
        #[arg(long, value_delimiter = ',')]
        track: Vec<String>,
        /// Albums (with their tracks) that move to the new artist.
        #[arg(long, value_delimiter = ',')]
        album: Vec<String>,
        /// The new artist's name; the same as the old one by default.
        #[arg(long)]
        name: Option<String>,
        /// The new artist's ID; made from the name by default.
        #[arg(long)]
        slug: Option<String>,
    },
    /// Change an artist's ID (the new one is normalized like any slug).
    Slug { artist: String, slug: String },
}

/// `album <id>` shows a profile; the subcommands manage albums.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct AlbumArgs {
    /// The album's ID (see `album list`).
    id: Option<String>,
    #[command(subcommand)]
    cmd: Option<AlbumCmd>,
}

#[derive(Subcommand)]
enum AlbumCmd {
    /// Show an album profile (for an album whose ID is also a subcommand name).
    Show { id: String },
    /// List albums, optionally one artist's or by the tags on the albums themselves.
    List {
        /// Only this artist's albums.
        #[arg(long)]
        artist: Option<String>,
        #[command(flatten)]
        tags: TagFilterArgs,
        #[arg(long, default_value = "")]
        query: String,
    },
    /// Add or remove tags on an album.
    Tag(TagEdit),
    /// Link an album to an artist profile.
    Link { album: String, artist: String },
    /// Remove an album's link to an artist profile.
    Unlink { album: String },
}

/// Tags to add to and remove from one thing.
#[derive(Args)]
struct TagEdit {
    /// The ID of the artist, album or playlist.
    id: String,
    #[arg(long, value_delimiter = ',', required_unless_present = "remove")]
    add: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    remove: Vec<String>,
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
    List {
        #[arg(long, default_value = "")]
        query: String,
        /// Named tag key; use --custom for unkeyed tags.
        #[arg(long, conflicts_with = "custom")]
        category: Option<String>,
        #[arg(long)]
        custom: bool,
        /// Direct children of this tag; pass an empty string for roots.
        #[arg(long)]
        parent: Option<String>,
    },
    Categories,
    /// Set a tag's parent, or clear it. Assignments do not change, only what the parent matches.
    Parent {
        name: String,
        #[arg(conflicts_with = "clear", required_unless_present = "clear")]
        parent: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Set how a frontend draws a tag: an icon (a key or path) and a colour (#rrggbb). Only the
    /// parts you give change; clear one with its --clear flag.
    Style {
        name: String,
        #[arg(long)]
        icon: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, conflicts_with = "icon")]
        clear_icon: bool,
        #[arg(long, conflicts_with = "color")]
        clear_color: bool,
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
    /// Index audio files without modifying them. Every artist it finds is new, never attached to
    /// one already in the library; afterwards it asks about new artists, albums and tracks that
    /// look like existing ones, and merges only what you say.
    Import {
        #[arg(required = true, num_args = 1..)]
        paths: Vec<PathBuf>,
        /// Do not ask about duplicates; review them later with `dedup`.
        #[arg(long)]
        disable_dedup_check: bool,
    },
    /// Measured listening statistics; --json includes every chart dataset.
    Stats(AnalyticsArgs),
    /// Previously played tracks inactive for 180 days, ordered by preference by default.
    Rediscover {
        #[command(flatten)]
        filter: FilterArgs,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Find and settle artists, albums and tracks that look alike.
    #[command(subcommand)]
    Dedup(DedupCmd),
    /// List tracks.
    List {
        #[command(flatten)]
        result: ResultArgs,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Search by tags, metadata and ratings.
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
    /// Everything about a track: metadata, tags, rating, file, artist, album, playlists, queue.
    Track { track_id: String },
    /// An artist profile (`artist <artist-id>`), or manage artists: list, tag, merge duplicates.
    Artist(ArtistArgs),
    /// An album profile (`album <album-id>`), or manage albums: list, tag, link to an artist.
    Album(AlbumArgs),
    /// Manage the tag vocabulary and hierarchy.
    #[command(subcommand)]
    Tag(TagCmd),
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
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=weave_player_lib::mock::MAX_COUNT as i64))]
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
    /// Save a dynamic playlist, or an empty curated playlist with rules for adding tracks.
    Create {
        name: String,
        /// What the playlist is for.
        #[arg(long)]
        description: Option<String>,
        /// Curate an ordered list with `playlist add/remove`; filters guide track selection in the GUI.
        #[arg(long)]
        manual: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Add or remove tags on a playlist.
    Tag(TagEdit),
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
    /// Change playlist membership rules (dynamic) or addition rules (manual); retain the others.
    Update {
        name: String,
        /// New description; pass "" to clear it.
        #[arg(long)]
        description: Option<String>,
        /// Stop drawing on other playlists: use the whole library again.
        #[arg(long, conflicts_with = "sources")]
        clear_sources: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Delete a playlist, after you type its name to confirm. Playlists that draw on it lose it as a source.
    Delete {
        name: String,
        /// Skip the question (for scripts): delete it, and remove it from the playlists that draw on it.
        #[arg(long, short = 'y')]
        yes: bool,
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
    List {
        #[arg(long, default_value = "")]
        query: String,
    },
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

/// The tag conditions, shared by every command that filters (tracks, artists, albums).
#[derive(Args, Default)]
struct TagFilterArgs {
    /// Require every tag: `mood:romantic,lang:spanish`, or by key `mood=romantic,calm`. Repeatable.
    /// Matches child tags too; `genre==bachata` (or `=genre:bachata`) matches that tag only.
    #[arg(long, value_name = "[KEYS=]TAGS", value_parser = parse_tag_cond)]
    tag_all: Vec<TagCond>,
    /// Require at least one tag: `lang:spanish,lang:french`, `genre=bachata,zouk`, `mood,vibe=party`.
    #[arg(long, value_name = "[KEYS=]TAGS", value_parser = parse_tag_cond)]
    tag_any: Vec<TagCond>,
    /// Exclude tracks with any of these tags: `live`, `mood=sad,angry`.
    #[arg(long, value_name = "[KEYS=]TAGS", value_parser = parse_tag_cond)]
    tag_exclude: Vec<TagCond>,
}

#[derive(Args, Default)]
struct FilterArgs {
    /// Start a filter clause: following filter flags belong to it until the next --filter-group.
    /// Flags before the first clause constrain all clauses. An optional JSON definition is also accepted.
    #[arg(long = "filter-group", value_name = "JSON", num_args = 0..=1,
        default_missing_value = "{}", value_parser = parse_filter_group)]
    groups: Vec<FilterSpec>,
    /// Start an OR tag alternative. BPM and other non-tag flags constrain all groups.
    #[arg(long, action = clap::ArgAction::Count)]
    or_group: u8,
    /// Start another AND block of OR tag alternatives.
    #[arg(long, action = clap::ArgAction::Count)]
    and_group: u8,
    /// Internal representation of flat AND/OR tag blocks.
    #[arg(long = "tag-block", hide = true, value_parser = parse_tag_block)]
    tag_blocks: Vec<Vec<weave_player_lib::core::TagFilter>>,
    /// Combine complete groups with AND (default) or OR.
    #[arg(long, default_value = "and")]
    group_join: weave_player_lib::core::FilterJoin,
    /// Draw only on these playlists (repeatable): a track in any of them, then the filters below on top. A manual playlist limits to its tracks, a dynamic one to what it matches now.
    #[arg(long = "from", value_name = "PLAYLIST")]
    sources: Vec<String>,
    #[command(flatten)]
    tags: TagFilterArgs,
    /// Minimum and maximum BPM of the track (inclusive; unknown never matches).
    #[arg(long)]
    bpm_min: Option<f64>,
    #[arg(long)]
    bpm_max: Option<f64>,
    /// Earliest and latest year of the track (inclusive).
    #[arg(long)]
    year_min: Option<i32>,
    #[arg(long)]
    year_max: Option<i32>,
    /// Only tracks first imported within this many rolling 24-hour days.
    #[arg(long)]
    imported_within_days: Option<u32>,
    /// Substring of title, artist or album.
    #[arg(long)]
    query: Option<String>,
    /// Tracks whose last start was at least N days ago (also matches never played).
    #[arg(long)]
    not_played_for_days: Option<u32>,
    /// Restrict to played (true) or never played (false) tracks.
    #[arg(long, num_args = 1)]
    has_played: Option<bool>,
    /// Case-insensitive part of the title. The genre is a tag: `--tag-any genre=bachata`.
    #[arg(long)]
    title: Option<String>,
    /// Case-insensitive part of the artist.
    #[arg(long)]
    artist: Option<String>,
    /// Case-insensitive part of the album.
    #[arg(long)]
    album: Option<String>,
    /// Minimum rating (1-5); unrated tracks never match.
    #[arg(long)]
    rating_min: Option<u8>,
    /// Maximum rating (1-5).
    #[arg(long)]
    rating_max: Option<u8>,
    #[command(flatten)]
    result: ResultArgs,
}

#[derive(Args)]
struct AnalyticsArgs {
    /// Rolling day (24h), week (7d), month (30d) or year (365d).
    #[arg(long, conflicts_with = "since_ms")]
    period: Option<weave_player_lib::core::AnalyticsPeriod>,
    /// Inclusive Unix millisecond lower bound (UTC).
    #[arg(long)]
    since_ms: Option<i64>,
    /// Exclusive Unix millisecond upper bound (UTC); defaults to now.
    #[arg(long)]
    until_ms: Option<i64>,
    #[arg(long, default_value_t = 10)]
    top_n: usize,
    /// Show measured events and clipped listening segments instead of aggregates.
    #[arg(long)]
    events: bool,
    #[command(flatten)]
    filter: FilterArgs,
}
impl AnalyticsArgs {
    fn query(self) -> weave_player_lib::core::AnalyticsQuery {
        weave_player_lib::core::AnalyticsQuery {
            since_ms: self.since_ms,
            until_ms: self.until_ms,
            period: self.period,
            filter: self.filter.into(),
            top_n: self.top_n,
        }
    }
}

/// One `--tag-*` value. Bare (`mood:calm,live`): plain tag names. Keyed (`lang=spanish,french`):
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
        return Err("expected tags, like `mood:calm,live` or `lang=spanish,french`".into());
    }
    Ok(TagCond {
        keys,
        values,
        exact,
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

impl TagFilterArgs {
    fn spec(self) -> FilterSpec {
        let (tag_all, mut tag_clauses) = split_conds(TagOp::All, self.tag_all);
        let (tag_any, any) = split_conds(TagOp::Any, self.tag_any);
        let (tag_exclude, none) = split_conds(TagOp::Exclude, self.tag_exclude);
        tag_clauses.extend(any);
        tag_clauses.extend(none);
        FilterSpec {
            tag_all,
            tag_any,
            tag_exclude,
            tag_clauses,
            ..Default::default()
        }
    }
}

fn parse_tag_block(text: &str) -> Result<Vec<weave_player_lib::core::TagFilter>, String> {
    let block: Vec<weave_player_lib::core::TagFilter> =
        serde_json::from_str(text).map_err(|e| e.to_string())?;
    let filter = weave_player_lib::core::TagFilter {
        blocks: vec![block.clone()],
        ..Default::default()
    };
    filter.validate().map_err(|e| e.to_string())?;
    Ok(block)
}

fn parse_filter_group(text: &str) -> Result<FilterSpec, String> {
    let filter: FilterSpec = serde_json::from_str(text).map_err(|error| error.to_string())?;
    filter.validate().map_err(|error| error.to_string())?;
    Ok(filter.normalized())
}

impl From<FilterArgs> for FilterSpec {
    fn from(a: FilterArgs) -> Self {
        let result = a.result.spec();
        FilterSpec {
            sources: a.sources,
            groups: a.groups,
            tag_blocks: a.tag_blocks,
            group_join: a.group_join,
            bpm_min: a.bpm_min,
            bpm_max: a.bpm_max,
            year_min: a.year_min,
            year_max: a.year_max,
            imported_within_days: a.imported_within_days,
            query: a.query,
            not_played_for_days: a.not_played_for_days,
            has_played: a.has_played,
            title: a.title,
            artist: a.artist,
            album: a.album,
            rating_min: a.rating_min,
            rating_max: a.rating_max,
            sort: result.sort,
            limit: result.limit,
            ..a.tags.spec()
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

/// Add and remove tags on an artist, album or playlist.
fn edit_tags(
    lib: &mut Library,
    kind: EntityKind,
    edit: TagEdit,
    json: bool,
) -> Result<(), Failure> {
    let added = if edit.add.is_empty() {
        0
    } else {
        lib.add_tags_to(kind, &edit.id, &edit.add)?
    };
    let removed = if edit.remove.is_empty() {
        0
    } else {
        lib.remove_tags_from(kind, &edit.id, &edit.remove)?
    };
    out(
        json,
        &serde_json::json!({ "added": added, "removed": removed }),
        || {
            outln!(
                "{kind} {}: added {added} tag(s), removed {removed}",
                edit.id
            )
        },
    )
}

/// Ask the user to type the playlist's name before deleting it. Without a terminal there is
/// nobody to ask, so it is refused with a pointer to `--yes`.
fn confirm_delete(message: &str, name: &str, json: bool) -> Result<(), Failure> {
    use std::io::IsTerminal;
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive {
        return Err(Error::InvalidInput(format!(
            "{message} There is no terminal to ask on; pass --yes to delete it"
        ))
        .into());
    }
    let confirmed = ask::confirm_by_name(
        message,
        name,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
    )
    .map_err(|e| Error::InvalidInput(e.to_string()))?;
    if !confirmed {
        return Err(
            Error::InvalidInput("the name did not match; nothing was deleted".into()).into(),
        );
    }
    Ok(())
}

/// Ask about artists, albums and tracks that look alike: those an import just brought (since
/// `mark`) or all of them. Without a terminal it only says how many there are.
fn review_similar(
    lib: &mut Library,
    kinds: &[EntityKind],
    mark: Option<&weave_player_lib::core::DedupMark>,
    json: bool,
) -> Result<(), Failure> {
    use std::io::IsTerminal;
    let mut waiting = 0;
    for kind in kinds {
        waiting += lib.duplicates(*kind, mark)?.len();
    }
    if waiting == 0 {
        return Ok(());
    }
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive {
        if !json {
            eprintln!("{waiting} possible duplicate(s); settle them with `weave-player dedup ask`");
        }
        return Ok(());
    }
    outln!();
    let o = dedup::review_all(
        lib,
        kinds,
        mark,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
    )?;
    outln!(
        "{} merged, {} kept separate, {} left for later",
        o.merged,
        o.kept_separate,
        o.skipped
    );
    Ok(())
}

fn default_db() -> PathBuf {
    weave_player_lib::paths::default_db()
}

/// `--mock-db empty` means `--mock-db=empty`: clap would otherwise take the next word as the
/// kind of an optional value, and a bare `--mock-db list` would read `list` as the kind.
fn mock_db_args(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let mut out: Vec<std::ffi::OsString> = Vec::new();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        if cfg!(debug_assertions)
            && a == "--mock-db"
            && args.peek().is_some_and(|n| n == "empty" || n == "seeded")
        {
            let mut joined = a;
            joined.push("=");
            joined.push(args.next().unwrap_or_default());
            out.push(joined);
        } else {
            out.push(a);
        }
    }
    out
}

fn parse_cli_from(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Cli, clap::Error> {
    let args = mock_db_args(args.into_iter());
    Cli::try_parse_from(clause_args::expand(args)?)
}

fn main() -> ExitCode {
    let cli = match parse_cli_from(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
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
    use weave_player_lib::core::PlayStatus;
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

/// The library to work on: `--mock-db`'s seeded dev database (debug builds), else `--db`, else
/// the default.
fn open_library(cli: &Cli) -> Result<(PathBuf, Library), Failure> {
    #[cfg(debug_assertions)]
    if let Some(kind) = cli.mock_db {
        let kind = match kind {
            MockDb::Seeded => weave_player_lib::mock::Mock::Seeded,
            MockDb::Empty => weave_player_lib::mock::Mock::Empty,
        };
        return Ok(weave_player_lib::mock::open_kind(kind)?);
    }
    let db = cli.db.clone().unwrap_or_else(default_db);
    let lib = Library::open(&db)?;
    Ok((db, lib))
}

fn run(cli: Cli) -> Result<(), Failure> {
    let json = cli.json;
    let show_ids = cli.show_ids;
    let (db, mut lib) = open_library(&cli)?;
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
        Cmd::Import {
            paths,
            disable_dedup_check,
        } => {
            let mark = lib.dedup_mark()?;
            let s = lib.import_paths(&paths)?;
            out(json, &s, || {
                outln!(
                    "imported {} new, {} already indexed, {} non-audio, {} issues",
                    s.added,
                    s.skipped,
                    s.ignored,
                    s.issues.len()
                );
                for issue in &s.issues {
                    outln!("{}: {}", issue.path, issue.message);
                }
            })?;
            if !disable_dedup_check {
                review_similar(&mut lib, &dedup::KINDS, Some(&mark), json)?;
            }
            if !s.issues.is_empty() {
                return Err(Failure::Other(
                    "import completed with inaccessible inputs; see report".into(),
                ));
            }
        }
        Cmd::List { result, page } => print_page(
            json,
            show_ids,
            page,
            &lib.search(&result.spec(), page.page())?,
        )?,
        Cmd::Stats(args) => {
            let events = args.events;
            let query = args.query();
            if events {
                let found = lib.listening_events(&query)?;
                out(json, &found, || {
                    for e in &found {
                        outln!(
                            "{}  {}  {} ms  {}",
                            e.id,
                            e.track,
                            e.listened_ms,
                            e.reason
                                .map(|r| r.to_string())
                                .unwrap_or_else(|| "unfinished".into())
                        );
                    }
                    if found.is_empty() {
                        outln!(
                            "No measured listening in this range. Statistics accumulate while audio plays; old start counts are not listening time."
                        );
                    }
                })?;
            } else {
                let stats = lib.listening_analytics(&query)?;
                out(json, &stats, || {
                    if stats.listens == 0 {
                        outln!(
                            "No measured listening in this range. Statistics accumulate while audio plays; old start counts are not listening time."
                        );
                    }
                    outln!(
                        "Listening hours: {:.3}\nUnique tracks: {}\nListens: {}\nCompleted listens: {}",
                        stats.listened_ms as f64 / 3_600_000.0,
                        stats.unique_tracks,
                        stats.listens,
                        stats.completed_listens
                    );
                    for (label, share) in [
                        ("Favorite track (rating)", &stats.favorite_track),
                        ("Favorite artist (mean rating)", &stats.favorite_artist),
                        ("Most listened track", &stats.most_listened_track),
                        ("Most listened artist", &stats.most_listened_artist),
                        ("Most listened tag", &stats.most_listened_tag),
                    ] {
                        outln!(
                            "{label}: {}",
                            share.as_ref().map(|s| s.name.as_str()).unwrap_or("—")
                        );
                    }
                    outln!(
                        "Longest session (listened ms): {}",
                        stats.longest_session.as_ref().map_or(0, |s| s.listened_ms)
                    );
                    outln!(
                        "Most replayed track: {}",
                        stats
                            .most_replayed_track
                            .as_ref()
                            .map_or("—", |s| s.track.as_str())
                    );
                    for l in &stats.loyalty {
                        outln!(
                            "{}: {} listens, {} completed, completion {}, replay {:.3}, repeats/day {:.3}",
                            l.track,
                            l.listens,
                            l.completed_listens,
                            l.completion_rate
                                .map(|r| format!("{r:.3}"))
                                .unwrap_or_else(|| "—".into()),
                            l.replay_rate.unwrap_or(0.0),
                            l.repeats_per_day
                        );
                    }
                })?;
            }
        }
        Cmd::Rediscover { filter, page } => print_page(
            json,
            show_ids,
            page,
            &lib.rediscover(&filter.into(), page.page())?,
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
        Cmd::Dedup(cmd) => match cmd {
            DedupCmd::List { kind } => {
                let kinds = kind.map_or(dedup::KINDS.to_vec(), |k| vec![k]);
                let mut pairs = Vec::new();
                for k in kinds {
                    pairs.extend(lib.duplicates(k, None)?);
                }
                profile::print_similar(json, &pairs)?;
            }
            DedupCmd::Ask { kind } => {
                let kinds = kind.map_or(dedup::KINDS.to_vec(), |k| vec![k]);
                review_similar(&mut lib, &kinds, None, json)?;
            }
            DedupCmd::Merge { kind, from, into } => {
                let m = lib.merge(kind, &from, &into)?;
                out(json, &m, || {
                    outln!(
                        "merged {kind} {from} into {into}: {} track(s), {} album(s) moved",
                        m.tracks,
                        m.albums
                    )
                })?;
            }
            DedupCmd::Separate { kind, a, b } => {
                lib.separate(kind, &a, &b)?;
                out(json, &serde_json::json!({ "different": [a, b] }), || {
                    outln!("{a} and {b} are different; they will not be suggested again")
                })?;
            }
        },
        Cmd::Track { track_id } => profile::print_track(json, &lib.track_detail(&track_id)?)?,
        Cmd::Artist(ArtistArgs { id: Some(id), .. })
        | Cmd::Artist(ArtistArgs {
            cmd: Some(ArtistCmd::Show { id }),
            ..
        }) => profile::print_artist(json, &lib.artist(&id)?)?,
        Cmd::Artist(ArtistArgs {
            cmd: None,
            id: None,
        }) => {
            return Err(Error::InvalidInput(
                "give an artist ID, or a subcommand (list, tag, duplicates, merge, separate)"
                    .into(),
            )
            .into());
        }
        Cmd::Artist(ArtistArgs { cmd: Some(cmd), .. }) => match cmd {
            ArtistCmd::Show { .. } => unreachable!("handled above"),
            ArtistCmd::List { tags, query } => {
                profile::print_artists(json, show_ids, &lib.search_artists(&query, &tags.spec())?)?
            }
            ArtistCmd::Tag(edit) => edit_tags(&mut lib, EntityKind::Artist, edit, json)?,
            ArtistCmd::Split {
                artist,
                track,
                album,
                name,
                slug,
            } => {
                let new =
                    lib.split_artist(&artist, &track, &album, name.as_deref(), slug.as_deref())?;
                out(json, &new, || {
                    outln!(
                        "split {artist}: {} ({}) now has {} track(s); the two are marked as different people",
                        new.name,
                        new.id,
                        new.stats.tracks
                    )
                })?;
            }
            ArtistCmd::Slug { artist, slug } => {
                let a = lib.set_artist_slug(&artist, &slug)?;
                out(json, &a, || outln!("{} is now {}", a.name, a.id))?;
            }
        },
        Cmd::Album(AlbumArgs { id: Some(id), .. })
        | Cmd::Album(AlbumArgs {
            cmd: Some(AlbumCmd::Show { id }),
            ..
        }) => profile::print_album(json, &lib.album(&id)?)?,
        Cmd::Album(AlbumArgs {
            cmd: None,
            id: None,
        }) => {
            return Err(Error::InvalidInput(
                "give an album ID, or a subcommand (list, tag, link, unlink)".into(),
            )
            .into());
        }
        Cmd::Album(AlbumArgs { cmd: Some(cmd), .. }) => match cmd {
            AlbumCmd::Show { .. } => unreachable!("handled above"),
            AlbumCmd::List {
                artist,
                tags,
                query,
            } => profile::print_albums(
                json,
                &lib.search_albums(&query, &tags.spec(), artist.as_deref())?,
            )?,
            AlbumCmd::Tag(edit) => edit_tags(&mut lib, EntityKind::Album, edit, json)?,
            AlbumCmd::Link { album, artist } => {
                let a = lib.link_album(&album, Some(&artist))?;
                out(json, &a, || outln!("{} is now linked to {artist}", a.title))?;
            }
            AlbumCmd::Unlink { album } => {
                let a = lib.link_album(&album, None)?;
                out(json, &a, || {
                    outln!("{} is no longer linked to an artist", a.title)
                })?;
            }
        },
        Cmd::Playlist(PlaylistCmd::Tag(edit)) => {
            edit_tags(&mut lib, EntityKind::Playlist, edit, json)?
        }
        Cmd::Tag(TagCmd::Create { name, parent }) => {
            let tag = lib.create_tag(&name, parent.as_deref())?;
            out(json, &tag, || outln!("created tag {}", tag.name))?;
        }
        Cmd::Tag(TagCmd::Categories) => {
            let keys = lib.tag_categories()?;
            out(json, &keys, || {
                for key in &keys {
                    outln!("{}", key.as_deref().unwrap_or("Custom"));
                }
            })?;
        }
        Cmd::Tag(TagCmd::List {
            query,
            category,
            custom,
            parent,
        }) => {
            let tags = lib.browse_tags(
                &query,
                if custom {
                    Some("")
                } else {
                    category.as_deref()
                },
                parent.as_deref(),
            )?;
            out(json, &tags, || {
                let rows = tags
                    .iter()
                    .map(|t| {
                        vec![
                            t.name.clone(),
                            t.parent.clone().unwrap_or_else(|| "-".into()),
                            t.icon.clone().unwrap_or_else(|| "-".into()),
                            t.color.clone().unwrap_or_else(|| "-".into()),
                            t.tracks.to_string(),
                            t.artists.to_string(),
                            t.albums.to_string(),
                            t.playlists.to_string(),
                        ]
                    })
                    .collect();
                print_columns(
                    &[
                        "TAG",
                        "PARENT",
                        "ICON",
                        "COLOR",
                        "TRACKS",
                        "ARTISTS",
                        "ALBUMS",
                        "PLAYLISTS",
                    ],
                    rows,
                    &[],
                    None,
                );
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
        Cmd::Tag(TagCmd::Style {
            name,
            icon,
            color,
            clear_icon,
            clear_color,
        }) => {
            let current = lib.tag(&name)?;
            let icon = if clear_icon {
                None
            } else {
                icon.or(current.icon)
            };
            let color = if clear_color {
                None
            } else {
                color.or(current.color)
            };
            let tag = lib.set_tag_style(&name, icon.as_deref(), color.as_deref())?;
            out(json, &tag, || {
                outln!(
                    "{}: icon {}, color {}",
                    tag.name,
                    tag.icon.as_deref().unwrap_or("-"),
                    tag.color.as_deref().unwrap_or("-")
                )
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
                lib.create_manual_playlist_with_filter(
                    &name,
                    description.as_deref(),
                    filter.into(),
                )?
            } else {
                lib.create_playlist(&name, description.as_deref(), filter.into())?
            };
            print_playlist(json, show_ids, &p)?;
        }
        Cmd::Playlist(PlaylistCmd::Update {
            name,
            description,
            clear_sources,
            filter,
        }) => {
            if clear_sources {
                lib.clear_playlist_sources(&name)?;
            }
            let p = lib.update_playlist(&name, description.as_deref(), filter.into())?;
            print_playlist(json, show_ids, &p)?;
        }
        Cmd::Playlist(PlaylistCmd::Delete { name, yes }) => {
            let playlist = lib.playlist(&name)?;
            let dependents = lib.playlist_dependents(&name)?;
            if !yes {
                let (_, found) = lib.playlist_tracks(&name, Page::numbered(1, 1))?;
                let message = ask::delete_message(&playlist, found.total, &dependents);
                confirm_delete(&message, &playlist.name, json)?;
            }
            let removed_from = lib.delete_playlist(&name, !dependents.is_empty())?;
            out(
                json,
                &serde_json::json!({
                    "deleted": playlist.id,
                    "removed_from": removed_from.iter().map(|p| &p.id).collect::<Vec<_>>(),
                }),
                || {
                    outln!("deleted playlist {}", playlist.name);
                    if !removed_from.is_empty() {
                        let names: Vec<&str> =
                            removed_from.iter().map(|p| p.name.as_str()).collect();
                        outln!("removed it from: {}", names.join(", "));
                    }
                },
            )?;
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
        Cmd::Playlist(PlaylistCmd::List { query }) => {
            print_playlists(json, show_ids, &lib.search_playlists(&query)?)?
        }

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
            let s = weave_player_lib::mock::seed(&mut lib, count as usize)?;
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
        parse_cli_from(
            std::iter::once("weave-player")
                .chain(args.iter().copied())
                .map(std::ffi::OsString::from),
        )
    }

    #[test]
    fn discovery_import_and_analytics_commands_parse() {
        for args in [
            vec![
                "weave-player",
                "import",
                "/music",
                "/song.wav",
                "--disable-dedup-check",
            ],
            vec![
                "weave-player",
                "artist",
                "list",
                "--query",
                "Alice",
                "--tag-all",
                "mood:calm",
            ],
            vec!["weave-player", "album", "list", "--query", "morning"],
            vec!["weave-player", "playlist", "list", "--query", "quiet"],
            vec![
                "weave-player",
                "tag",
                "list",
                "--category",
                "mood",
                "--query",
                "calm",
            ],
            vec!["weave-player", "tag", "list", "--custom"],
            vec!["weave-player", "tag", "categories"],
            vec![
                "weave-player",
                "rediscover",
                "--not-played-for-days",
                "90",
                "--sort",
                "rating",
                "--desc",
            ],
            vec![
                "weave-player",
                "stats",
                "--period",
                "week",
                "--tag-exclude",
                "live",
                "--json",
            ],
            vec![
                "weave-player",
                "stats",
                "--since-ms",
                "1000",
                "--until-ms",
                "5000",
                "--events",
            ],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }
        assert!(
            Cli::try_parse_from([
                "weave-player",
                "stats",
                "--period",
                "day",
                "--since-ms",
                "1"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "weave-player",
                "tag",
                "list",
                "--custom",
                "--category",
                "mood"
            ])
            .is_err()
        );
        let Cmd::Import { paths, .. } = Cli::try_parse_from(["weave-player", "import", "/a", "/b"])
            .unwrap()
            .command
        else {
            panic!("import")
        };
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn cli_complete_groups_match_romantic_slow_or_party_fast_in_the_engine() {
        use weave_player_lib::core::{FilterJoin, Metadata, ScannedTrack};
        let romantic = r#"{"tag_all":["mood:romantic"],"bpm_max":90}"#;
        let party = r#"{"tag_all":["mood:party"],"bpm_min":120}"#;
        let Cmd::Search { filter, .. } = parse(&[
            "search",
            "--group-join",
            "or",
            "--filter-group",
            romantic,
            "--filter-group",
            party,
        ])
        .unwrap()
        .command
        else {
            panic!("search");
        };
        let filter = FilterSpec::from(filter);
        assert_eq!(filter.group_join, FilterJoin::Or);
        assert_eq!(filter.groups.len(), 2);
        let mut library = Library::open_in_memory().unwrap();
        for (title, mood, bpm) in [
            ("a", "romantic", 80.0),
            ("b", "romantic", 140.0),
            ("c", "party", 80.0),
            ("d", "party", 140.0),
        ] {
            library
                .import_scanned(&[ScannedTrack {
                    path: format!("/{title}"),
                    metadata: Metadata {
                        title: title.into(),
                        artist: Some("x".into()),
                        bpm: Some(bpm),
                        ..Default::default()
                    },
                }])
                .unwrap();
            library
                .add_tags(&format!("x-{title}"), &[format!("mood:{mood}")])
                .unwrap();
        }
        library
            .create_playlist("Grouped dynamic", None, filter.clone())
            .unwrap();
        assert_eq!(
            library
                .playlist_tracks("Grouped dynamic", Page::ALL)
                .unwrap()
                .1
                .total,
            2
        );
        let results = library.search(&filter, Page::ALL).unwrap();
        assert_eq!(
            results
                .tracks
                .iter()
                .map(|track| track.metadata.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "d"]
        );
        let Cmd::Search { filter, .. } = parse(&[
            "search",
            "--filter-group",
            romantic,
            "--filter-group",
            party,
        ])
        .unwrap()
        .command
        else {
            panic!("search");
        };
        let filter = FilterSpec::from(filter);
        assert_eq!(filter.group_join, FilterJoin::And);
        assert!(
            library
                .search(&filter, Page::ALL)
                .unwrap()
                .tracks
                .is_empty()
        );
        let Cmd::Playlist(PlaylistCmd::Create { manual, filter, .. }) = parse(&[
            "playlist",
            "create",
            "Curated",
            "--manual",
            "--group-join",
            "or",
            "--filter-group",
            romantic,
            "--filter-group",
            party,
        ])
        .unwrap()
        .command
        else {
            panic!("create");
        };
        assert!(manual);
        library
            .create_manual_playlist_with_filter("Curated", None, filter.into())
            .unwrap();
        assert_eq!(
            library
                .playlist_add_candidates("Curated", Page::ALL)
                .unwrap()
                .total,
            2
        );
        assert_eq!(
            library
                .playlist_tracks("Curated", Page::ALL)
                .unwrap()
                .1
                .total,
            0
        );
        assert!(parse(&["search", "--filter-group", "not json"]).is_err());
        assert!(
            parse(&[
                "search",
                "--filter-group",
                r#"{"bpm_min":120,"bpm_max":90}"#
            ])
            .is_err()
        );
        assert!(parse(&["search", "--group-join", "xor"]).is_err());
        let Cmd::Search { filter, .. } = parse(&["search", "--filter-group", r#"{"limit":1}"#])
            .unwrap()
            .command
        else {
            panic!("search");
        };
        assert!(FilterSpec::from(filter).validate().is_err());
    }

    #[test]
    fn flat_tag_groups_keep_bpm_global_and_chain_with_and() {
        use weave_player_lib::core::{Metadata, ScannedTrack};
        let command = [
            "search",
            "--or-group",
            "--tag-any",
            "genre:zouk,genre:bachata",
            "--tag-all",
            "mood:romantic",
            "--or-group",
            "--tag-all",
            "lang:tamil,tag:romantic,90s",
            "--bpm-max",
            "90",
        ];
        let Cmd::Search { filter, .. } = parse(&command).unwrap().command else {
            panic!("search")
        };
        let filter = FilterSpec::from(filter).normalized();
        assert_eq!(filter.bpm_max, Some(90.0));
        assert_eq!(filter.tag_blocks.len(), 1);
        assert_eq!(filter.tag_blocks[0].len(), 2);
        assert!(filter.groups.is_empty());
        let mut library = Library::open_in_memory().unwrap();
        for (title, tags, bpm) in [
            ("a", vec!["genre:zouk", "mood:romantic"], 80.0),
            ("b", vec!["genre:bachata", "mood:romantic"], 100.0),
            ("c", vec!["lang:tamil", "tag:romantic", "90s"], 90.0),
            ("d", vec!["lang:tamil", "tag:romantic"], 80.0),
            ("e", vec!["genre:zouk"], 80.0),
        ] {
            library
                .import_scanned(&[ScannedTrack {
                    path: format!("/{title}"),
                    metadata: Metadata {
                        title: title.into(),
                        artist: Some("x".into()),
                        bpm: Some(bpm),
                        ..Default::default()
                    },
                }])
                .unwrap();
            library
                .add_tags(
                    &format!("x-{title}"),
                    &tags.into_iter().map(str::to_string).collect::<Vec<_>>(),
                )
                .unwrap();
        }
        let found = library.search(&filter, Page::ALL).unwrap();
        assert_eq!(
            found
                .tracks
                .iter()
                .map(|t| t.metadata.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        library
            .create_playlist("Flat blocks", None, filter.clone())
            .unwrap();
        assert_eq!(
            library
                .playlist_tracks("Flat blocks", Page::ALL)
                .unwrap()
                .1
                .total,
            2
        );
        let mut chained = command.to_vec();
        chained.extend([
            "--and-group",
            "--tag-all",
            "lang:tamil",
            "--or-group",
            "--tag-all",
            "lang:french",
        ]);
        let Cmd::Search { filter, .. } = parse(&chained).unwrap().command else {
            panic!("search")
        };
        let filter = FilterSpec::from(filter).normalized();
        assert_eq!(filter.tag_blocks.len(), 2);
        let found = library.search(&filter, Page::ALL).unwrap();
        assert_eq!(
            found
                .tracks
                .iter()
                .map(|t| t.metadata.title.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        let description = output::describe(&filter);
        assert!(description.contains("--and-group"));
        assert!(description.contains("--bpm-max 90"));
        assert!(parse(&["search", "--or-group", "--tag-all"]).is_err());
        assert!(parse(&["search", "--tag-block", "bad JSON"]).is_err());
    }

    #[test]
    fn recent_imports_cli_uses_the_shared_filter() {
        let Cmd::Search { filter, .. } = Cli::try_parse_from([
            "weave-player",
            "search",
            "--imported-within-days",
            "7",
            "--artist",
            "x",
        ])
        .unwrap()
        .command
        else {
            panic!("not search")
        };
        let f = FilterSpec::from(filter);
        assert_eq!(f.imported_within_days, Some(7));
        assert_eq!(f.artist.as_deref(), Some("x"));
        assert!(output::describe(&f).contains("--imported-within-days 7"));
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
            "--bpm-min",
            "90",
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
            (spec.tag_all, spec.bpm_min, spec.limit),
            (vec!["bachata".into(), "90s".into()], Some(90.0), Some(20))
        );
        assert!(
            matches!(
                parse(&["play", "random"]).unwrap().command,
                Cmd::Play(PlayCmd::Random { .. })
            ),
            "filters are optional"
        );
        assert!(parse(&["play", "random", "--rating-min", "4"]).is_ok());
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
            "--bpm-min",
            "90",
            "--year-max",
            "1999",
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
        assert_eq!((f.bpm_min, f.year_max), (Some(90.0), Some(1999)));
        assert_eq!(
            output::describe(&f),
            "--tag-all practice,90s --tag-all genre,style==salsa --tag-any genre=bachata,sals \
             --tag-exclude period=80s --bpm-min 90 --year-max 1999"
        );
        assert!(parse(&["search", "--tag-any", "genre="]).is_err());
        assert!(
            parse(&["search", "--tag-range", "tempo=90..120"]).is_err(),
            "tags are text; numbers are the BPM and year columns"
        );
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
    fn playlists_can_draw_on_other_playlists() {
        let Cmd::Search { filter, .. } = parse(&[
            "search",
            "--from",
            "road-trip",
            "--from",
            "Slow nights",
            "--tag-all",
            "lang:spanish",
        ])
        .unwrap()
        .command
        else {
            panic!("not a search")
        };
        let f = FilterSpec::from(filter);
        assert_eq!(f.sources, ["road-trip", "Slow nights"]);
        assert_eq!(
            output::describe(&f),
            "--from road-trip --from Slow nights --tag-all lang:spanish"
        );
        let Cmd::Playlist(PlaylistCmd::Update { clear_sources, .. }) =
            parse(&["playlist", "update", "mix", "--clear-sources"])
                .unwrap()
                .command
        else {
            panic!("not an update")
        };
        assert!(clear_sources);
        assert!(
            parse(&[
                "playlist",
                "update",
                "mix",
                "--clear-sources",
                "--from",
                "x"
            ])
            .is_err(),
            "clear or replace, not both"
        );
    }

    #[test]
    fn deleting_a_playlist_takes_a_name_and_an_optional_yes() {
        let Cmd::Playlist(PlaylistCmd::Delete { name, yes }) =
            parse(&["playlist", "delete", "mix"]).unwrap().command
        else {
            panic!("not a delete")
        };
        assert_eq!((name.as_str(), yes), ("mix", false));
        assert!(matches!(
            parse(&["playlist", "delete", "mix", "--yes"])
                .unwrap()
                .command,
            Cmd::Playlist(PlaylistCmd::Delete { yes: true, .. })
        ));
        assert!(parse(&["playlist", "delete"]).is_err());
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
