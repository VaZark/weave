use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use weave_core::{Error, FilterSpec, Page, SearchResult, Track};
use weave_engine::{Library, SmartPlaylist};

/// Exit codes: 0 ok, 1 general failure, 2 usage, 3 not found, 4 invalid input/filter, 5 unsupported.
#[derive(Parser)]
#[command(name = "weave", version, about = "Local-first music library with tags and smart playlists")]
struct Cli {
    /// Library database path.
    #[arg(long, global = true, env = "WEAVE_DB")]
    db: Option<PathBuf>,
    /// Emit structured JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Index audio files without modifying them.
    Import { path: PathBuf },
    /// List tracks.
    List(PageArgs),
    /// Search by tags, BPM and metadata.
    Search {
        #[command(flatten)]
        filter: FilterArgs,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Add tags to a track (or the current track). Tracks are addressed by their ID (slug).
    Add { track_id: Option<String>, #[arg(long = "tag", required = true, value_delimiter = ',')] tags: Vec<String> },
    /// Remove tags from a track.
    Remove { track_id: String, #[arg(long = "tag", required = true, value_delimiter = ',')] tags: Vec<String> },
    /// Show a track's tags.
    Tags { track_id: String },
    /// Manage smart playlists.
    #[command(subcommand)]
    Playlist(PlaylistCmd),
    /// Inspect backends.
    #[command(subcommand)]
    Backend(BackendCmd),
    /// Play a track (0.a2).
    Play { track_id: String },
    /// Pause playback (0.a2).
    Pause,
    /// Resume playback (0.a2).
    Resume,
    /// Skip to the next track (0.a2).
    Next,
    /// Show the current track (0.a2).
    Now,
}

#[derive(Subcommand)]
enum PlaylistCmd {
    /// Save a dynamic playlist: filters, not membership.
    Create { name: String, #[command(flatten)] filter: FilterArgs },
    /// Evaluate a playlist's saved filters.
    Show {
        name: String,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Change the specified filters; retain the others.
    Update { name: String, #[command(flatten)] filter: FilterArgs },
    /// Queue the current matches (0.a2).
    Play { name: String },
    /// List saved playlists.
    List,
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
    limit: u32,
    /// Print every match instead of one page.
    #[arg(long, conflicts_with_all = ["page", "limit"])]
    all: bool,
}

impl PageArgs {
    fn page(self) -> Page {
        if self.all { Page::ALL } else { Page::numbered(self.page as usize, self.limit as usize) }
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
        }
        .normalized()
    }
}

enum Failure {
    Engine(Error),
    Unsupported(&'static str),
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
                        Error::InvalidFilter(_) | Error::InvalidInput(_) | Error::AlreadyExists(_) => 4,
                        _ => 1,
                    },
                    e.to_string(),
                ),
                Failure::Unsupported(m) => (5, m.to_string()),
                Failure::Other(m) => (1, m),
            };
            eprintln!("weave: {msg}");
            ExitCode::from(code)
        }
    }
}

const NOT_YET: &str = "playback is not implemented yet (planned for 0.a2)";

fn run(cli: Cli) -> Result<(), Failure> {
    // Reject commands that cannot work before touching the database.
    match &cli.command {
        Cmd::Play { .. } | Cmd::Pause | Cmd::Resume | Cmd::Next | Cmd::Now | Cmd::Playlist(PlaylistCmd::Play { .. }) => {
            return Err(Failure::Unsupported(NOT_YET));
        }
        Cmd::Add { track_id: None, .. } => {
            return Err(Failure::Unsupported("no active playback session; pass a track id (current-track tagging arrives with 0.a2)"));
        }
        _ => {}
    }

    let json = cli.json;
    let mut lib = Library::open(&cli.db.unwrap_or_else(default_db))?;

    match cli.command {
        Cmd::Import { path } => {
            let s = lib.import(&path)?;
            out(json, &serde_json::json!({ "added": s.added, "skipped": s.skipped }), || {
                println!("imported {} new, {} already indexed", s.added, s.skipped)
            })?;
        }
        Cmd::List(p) => print_page(json, p, &lib.search(&FilterSpec::default(), p.page())?)?,
        Cmd::Search { filter, page } => print_page(json, page, &lib.search(&filter.into(), page.page())?)?,
        Cmd::Add { track_id: Some(id), tags } => {
            let n = lib.add_tags(&id, &tags)?;
            out(json, &serde_json::json!({ "added": n }), || println!("added {n} tag(s)"))?;
        }
        Cmd::Remove { track_id, tags } => {
            let n = lib.remove_tags(&track_id, &tags)?;
            out(json, &serde_json::json!({ "removed": n }), || println!("removed {n} tag(s)"))?;
        }
        Cmd::Tags { track_id } => {
            let tags = lib.tags(&track_id)?;
            out(json, &tags, || tags.iter().for_each(|t| println!("{t}")))?;
        }
        Cmd::Playlist(PlaylistCmd::Create { name, filter }) => {
            let p = lib.create_playlist(&name, filter.into())?;
            print_playlist(json, &p)?;
        }
        Cmd::Playlist(PlaylistCmd::Update { name, filter }) => {
            let p = lib.update_playlist(&name, filter.into())?;
            print_playlist(json, &p)?;
        }
        Cmd::Playlist(PlaylistCmd::Show { name, page }) => {
            let (_, result) = lib.playlist_tracks(&name, page.page())?;
            print_page(json, page, &result)?;
        }
        Cmd::Playlist(PlaylistCmd::List) => {
            let ps = lib.playlists()?;
            if json {
                print_json(&ps.iter().map(|p| serde_json::json!({ "name": p.name, "filter": p.filter })).collect::<Vec<_>>())?;
            } else {
                ps.iter().for_each(|p| println!("{}\t{}", p.name, describe(&p.filter)));
            }
        }
        Cmd::Backend(BackendCmd::List) => {
            let bs = lib.backends();
            if json {
                print_json(&bs.iter().map(|b| serde_json::json!({ "id": b.id, "capabilities": b.capabilities })).collect::<Vec<_>>())?;
            } else {
                bs.iter().for_each(|b| println!("{}\t{} capabilities", b.id, b.capabilities.len()));
            }
        }
        Cmd::Backend(BackendCmd::Show { id }) => {
            let b = lib.backend(&id)?;
            if json {
                print_json(&serde_json::json!({ "id": b.id, "capabilities": b.capabilities }))?;
            } else {
                println!("{}", b.id);
                for c in &b.capabilities {
                    println!(
                        "  {:?}/{}\tsupported={} authorized={} available={}",
                        c.group, c.name, c.state.supported, c.state.authorized, c.state.available
                    );
                }
            }
        }
        Cmd::Add { track_id: None, .. }
        | Cmd::Play { .. }
        | Cmd::Pause
        | Cmd::Resume
        | Cmd::Next
        | Cmd::Now
        | Cmd::Playlist(PlaylistCmd::Play { .. }) => unreachable!("rejected above"),
    }
    Ok(())
}

fn print_json<T: Serialize>(v: &T) -> Result<(), Failure> {
    let s = serde_json::to_string_pretty(v).map_err(|e| Failure::Other(e.to_string()))?;
    println!("{s}");
    Ok(())
}

fn out<T: Serialize>(json: bool, v: &T, text: impl FnOnce()) -> Result<(), Failure> {
    if json { print_json(v) } else { text(); Ok(()) }
}

fn print_page(json: bool, args: PageArgs, r: &SearchResult) -> Result<(), Failure> {
    let (page, per_page) = if args.all { (1, r.total.max(1)) } else { (args.page as usize, args.limit as usize) };
    let pages = r.total.div_ceil(per_page).max(1);
    if json {
        return print_json(&serde_json::json!({
            "total": r.total, "page": page, "pages": pages, "per_page": per_page, "tracks": r.tracks,
        }));
    }
    print_table(&r.tracks);
    // Footer goes to stderr so stdout stays a clean table.
    if !args.all && pages > 1 {
        let hint = if page < pages { format!("; next: --page {}", page + 1) } else { String::new() };
        eprintln!("page {page} of {pages} ({} tracks){hint}", r.total);
    } else {
        eprintln!("{} track(s)", r.total);
    }
    Ok(())
}

fn print_table(tracks: &[Track]) {
    const HEADERS: [&str; 9] = ["ID", "TITLE", "ARTIST", "ALBUM", "GENRE", "YEAR", "BPM", "TIME", "TAGS"];
    let dash = || "-".to_string();
    let rows: Vec<[String; 9]> = tracks
        .iter()
        .map(|t| {
            let m = &t.metadata;
            [
                t.id.0.clone(),
                m.title.clone(),
                m.artist.clone().unwrap_or_else(dash),
                m.album.clone().unwrap_or_else(dash),
                m.genre.clone().unwrap_or_else(dash),
                m.year.map_or_else(dash, |y| y.to_string()),
                m.bpm.map_or_else(dash, |b| format!("{b}")),
                m.duration_ms.map_or_else(dash, |ms| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)),
                if t.tags.is_empty() { dash() } else { t.tags.join(",") },
            ]
        })
        .collect();

    const MAX: usize = 32;
    // The ID column is never truncated: it is meant to be copied.
    let width = |i: usize| {
        let w = rows.iter().map(|r| r[i].chars().count()).chain([HEADERS[i].len()]).max().unwrap();
        if i == 0 { w } else { w.min(MAX) }
    };
    let widths: Vec<usize> = (0..9).map(width).collect();
    let line = |cells: &[&str]| {
        let parts: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| {
                let c = if c.chars().count() > *w {
                    c.chars().take(w - 1).chain(['…']).collect::<String>()
                } else {
                    c.to_string()
                };
                format!("{c:<w$}")
            })
            .collect();
        println!("{}", parts.join("  ").trim_end());
    };
    line(&HEADERS);
    for r in &rows {
        line(&r.each_ref().map(String::as_str));
    }
}

fn print_playlist(json: bool, p: &SmartPlaylist) -> Result<(), Failure> {
    out(json, &serde_json::json!({ "name": p.name, "filter": p.filter }), || {
        println!("{}\t{}", p.name, describe(&p.filter))
    })
}

fn describe(f: &FilterSpec) -> String {
    let mut parts = Vec::new();
    for (k, v) in [("tag-all", &f.tag_all), ("tag-any", &f.tag_any), ("tag-none", &f.tag_none)] {
        if !v.is_empty() {
            parts.push(format!("--{k} {}", v.join(",")));
        }
    }
    if let Some(v) = f.bpm_min { parts.push(format!("--bpm-min {v}")); }
    if let Some(v) = f.bpm_max { parts.push(format!("--bpm-max {v}")); }
    if let Some(v) = f.year_min { parts.push(format!("--year-min {v}")); }
    if let Some(v) = f.year_max { parts.push(format!("--year-max {v}")); }
    for (k, v) in [("title", &f.title), ("artist", &f.artist), ("album", &f.album), ("genre", &f.genre)] {
        if let Some(v) = v { parts.push(format!("--{k} {v:?}")); }
    }
    if parts.is_empty() { "(all tracks)".into() } else { parts.join(" ") }
}
