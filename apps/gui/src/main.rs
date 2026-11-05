//! Weave's GUI frontend: iced with iced-adaptive-design (`iad`). See `docs/ui`.
//!
//! The UI runs on the engine: `data.rs` is a snapshot of the library database and `backend.rs`
//! drives the queue and transport. The application state (destination, search, jam, queue) lives
//! here; the layouts in `ui.rs` only render it, so a
//! window that crosses the breakpoint loses nothing.

mod backend;
mod components;
mod data;
mod icons;
mod platforms;
mod ui;
mod ui_dialogs;
mod ui_lists;
mod ui_minimal;
mod ui_player;

use std::time::{Duration, Instant};

use data::{Filter, Library, Playlist, RecentOrder, Track};
use iad::adaptive::{Adaptive, Layouts};
use iad::breakpoints::WidthClass;
use iad::layouts::virtualize::Viewport;
use iad::primitives::history::History;
use iad::primitives::reorder::{Event as ReorderEvent, ReorderState};
use iad::primitives::sorting::{SortState, cmp_str};
use iad::primitives::synced::{self, Line};
use iad::transition::Event;
use iced::{Subscription, Task};
use iced_adaptive_design as iad;

type Element<'a> = iad::Element<'a, Message>;

const JAM_LIST: f32 = 240.0;

/// Row heights of every track list: the normal one carries a picture and two lines.
pub const ROW: f32 = 56.0;
pub const ROW_COMPACT: f32 = 32.0;

const WINDOW: (f32, f32) = (1120.0, 720.0);

/// The window in minimal mode: one line, as high as the player bar.
const MINIMAL: (f32, f32) = (560.0, 72.0);
/// The narrowest minimal mode goes.
const MINIMAL_MIN_WIDTH: f32 = 360.0;
const MINIMAL_MAX_WIDTH: f32 = 900.0;

/// `WEAVE_GUI_SIZE=WxH` starts the window at that size (to judge the narrow layout).
fn window_size() -> (f32, f32) {
    std::env::var("WEAVE_GUI_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(WINDOW)
}

/// The layouts, each built and judged alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Narrow,
    Wide,
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Layout::Narrow => "Narrow",
            Layout::Wide => "Wide",
        })
    }
}

/// Where the user is. Layouts map their triggers (tabs, sidebar rows, pills) to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    Home,
    /// Rediscover and explore: old favourites, what you play most, what you might like, and
    /// links to every list.
    Explore,
    Recent,
    Tracks,
    Artists,
    Albums,
    Playlists,
    Playlist(usize),
    /// Index into `Library::artists()`.
    Artist(usize),
    /// Index into `Library::albums()`.
    Album(usize),
}

impl Place {
    pub fn home() -> Self {
        Self::at(Dest::Home)
    }
    pub fn at(dest: Dest) -> Self {
        Self {
            dest,
            searching: false,
            query: String::new(),
        }
    }
}

/// Where the user was: what Back returns to. Detail pages are opened from anywhere (Home, a
/// search, another detail page), so Back is the previous place, not a fixed parent.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub dest: Dest,
    pub searching: bool,
    pub query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The desktop switched between light and dark; the theme is read again on redraw.
    SystemScheme(iad::Scheme),
    // Resizing and the transitions between layouts.
    Resized(f32, f32),
    /// An animation frame (while a sheet or the queue slides).
    Frame(Instant),
    Tick(Instant),
    /// The engine's session, read again.
    Poll,
    Transition(Event<Layout>),
    Window(iad::windows::Action),
    // Actions: every trigger fires one of these.
    /// Go to a top-level place: the history starts over.
    Go(Dest),
    /// Open a page from where you are: Back returns here.
    Open(Dest),
    SearchOpen(bool),
    SearchText(String),
    /// The jam is a page: this shows it, or goes back to the page you were on.
    ToggleJam,
    ToggleExplore,
    /// The menu's Import items: choose files (`true`) or a folder in the system file chooser.
    Import(bool),
    /// What the file chooser returned (nothing when it was cancelled).
    Picked(Vec<std::path::PathBuf>),
    /// The import finished: what to tell the user.
    Imported(Result<String, String>),
    /// The bottom sheet of a menu (narrow): a track's actions or a page's options.
    OpenSheet(Option<Sheet>),
    /// How Recent tracks is ordered: by play or by when added.
    SetRecentOrder(RecentOrder),
    /// How the open playlist or album is played: shuffled, in order, or as the player says.
    SetShufflePref(Option<bool>),
    /// A choice made in the sheet: it closes, then does it.
    SheetAction(Box<Message>),
    SetDensity(DensityPref),
    WhileDays(u32),
    ToggleFavorite(String),
    /// The heart of a track: in or out of the Favourites playlist.
    ToggleFavoriteTrack(usize),
    SleepTimer(Option<u32>),
    ListScroll(Viewport),
    RateTrack(usize),
    SetRating(usize, u8),
    CloseRate,
    InfoTrack(usize),
    CloseInfo,
    QueueRemove(usize),
    /// The queue's Clear: drops what follows the playing track.
    ClearQueue,
    /// A history entry of the queue: play it again (its place in the whole queue).
    QueueJump(usize),
    ToggleQueueHistory,
    ToggleUpNext,
    NowPlaying(bool),
    ToggleQueue,
    /// Full screen mode on or off: the window goes fullscreen and shows Now Playing; leaving
    /// puts both back.
    Fullscreen(bool),
    /// F11.
    ToggleFullscreen,
    /// Escape: leaves fullscreen, and does nothing otherwise.
    EscapeKey,
    ToggleLyrics,
    // The filter page, shared by search and jam.
    ToggleTag(String),
    TagQuery(String),
    Bpm((f32, f32)),
    ClearFilter,
    Surprise,
    StartJam,
    // Library.
    Sort(Col),
    /// Plays the rows of the page from this track.
    /// A track of the page's list: the queue stays, and the highlight moves to it (the page's
    /// list becomes the queue only when nothing is queued).
    PlayTrack(usize),
    /// A single track from a short list: the same, with nothing else to queue.
    PlayOne(usize),
    PlayAll(Vec<usize>),
    /// Stores the jam's filters as a dynamic playlist.
    SaveJamAsPlaylist,
    JamScroll(Viewport),
    // A track's menu.
    PlayNext(usize),
    AddToQueue(usize),
    ViewAlbum(usize),
    ViewArtist(usize),
    EditTrack(usize),
    EditToggleTag(String),
    SaveTrack,
    CancelTrack,
    /// Narrow layout: open or close the filters above the matches.
    JamFilters(bool),
    Back,
    Describe(String),
    /// The pencil of a detail page: its name (a playlist's) and its description are edited together
    /// in a form.
    EditDetails,
    /// Save and Cancel of the form.
    SaveDetails,
    CancelDetails,
    /// What is typed in the form's description field.
    DescInput(String),
    /// The playlist's name, as typed in its Edit mode.
    NameInput(String),
    DescExpanded(bool),
    EditFilter,
    DraftToggle(String),
    DraftBpm((f32, f32)),
    SaveFilter,
    CancelFilter,
    Days(u8),
    // The player.
    TogglePlay,
    Next,
    Previous,
    Seek(f32),
    Shuffle,
    Repeat,
    /// The volume slider: 0.0 to 1.0.
    SetVolume(f32),
    /// Enter (true) or leave minimal mode: the window shrinks to the one-line player and back.
    Minimal(bool),
    /// A new reading of the spectrum, while minimal mode draws it.
    SpectrumTick,
    /// The cover in minimal mode: switch between the spectrum and the song with its controls.
    MinimalDetails,
    /// The volume button: mute, or back to the volume before.
    ToggleMute,
    Queue(ReorderEvent),
    Progress,
    // Development: `ICED_ADAPTIVE_DESIGN_SCREENSHOT=file.png` saves a screenshot and quits;
    // `WEAVE_GUI_START=jam|search|library|artists|popover|playlist|dialog|artist|album|playing|queue|lyrics` opens that state first.
    Captured(Result<std::path::PathBuf, String>),
}

/// The track whose tags are being edited in the dialog.
#[derive(Debug, Clone)]
pub struct TrackEdit {
    pub id: usize,
    pub tags: Vec<String>,
}

/// What the bottom sheet (narrow) shows: a menu that is a dropdown on desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    /// A track's actions: the track, and whether "View album" shows.
    Track(usize, bool),
    /// A detail page's options: whether the page is a dynamic playlist.
    Options(bool),
    /// The order of Recent tracks.
    Recent,
}

/// The Edit form of a detail page: the name (a playlist's) and the description, typed until Save.
#[derive(Debug, Clone, PartialEq)]
pub struct DetailsDraft {
    pub name: Option<String>,
    pub description: String,
}

impl Player {
    /// What comes after the track playing now, as an always-editable list. The playing track is
    /// not in it: it is shown on its own, so it cannot be dragged or removed.
    fn sync_up_next(&mut self) {
        let from = self.current.map_or(0, |c| c + 1).min(self.queue.len());
        self.up_next = editing_queue(self.queue[from..].to_vec());
    }
}

/// How dense the app's lists are: the desktop's setting (`[appearance] density`) unless the user
/// chooses one here. This is the app's own preference, not the desktop config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DensityPref {
    #[default]
    Desktop,
    Default,
    Compact,
}

impl DensityPref {
    pub const ALL: [DensityPref; 3] = [
        DensityPref::Desktop,
        DensityPref::Default,
        DensityPref::Compact,
    ];
}

impl std::fmt::Display for DensityPref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DensityPref::Desktop => "Follow the desktop",
            DensityPref::Default => "Default",
            DensityPref::Compact => "Compact",
        })
    }
}

/// The queue's opening animation: iad's `Progress`, driven each frame, so iad's dev switch that
/// holds animations holds this one too. `shown` is 0 (closed) to 1 (open), already eased.
#[derive(Debug, Clone)]
pub struct Slide {
    progress: iad::animation::Progress,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            progress: iad::animation::Progress::new(0.0),
        }
    }
}

impl Slide {
    pub fn shown(&self) -> f32 {
        self.progress.shown
    }

    /// Moves toward open or closed as of `now`; `true` while it is still moving.
    pub fn drive(&mut self, open: bool, now: Instant) -> bool {
        let spec = if iad::animation::enabled() {
            iad::animation::Spec::Timed(
                Duration::from_millis(280),
                iad::animation::Easing::EaseOutCubic,
            )
        } else {
            iad::animation::Spec::Instant
        };
        self.progress.drive(if open { 1.0 } else { 0.0 }, now, spec)
    }

    /// Whether it is on screen at all (opening, open or closing).
    pub fn mounted(&self) -> bool {
        self.progress.shown > 0.0
    }

    pub fn is_open_at(&self, open: bool) -> bool {
        self.progress.shown == if open { 1.0 } else { 0.0 }
    }
}

pub struct Player {
    /// The whole queue, in order; `current` indexes it.
    pub queue: Vec<usize>,
    /// The part after `current`, which the queue page lists, reorders and removes from.
    pub up_next: ReorderState<usize>,
    pub current: Option<usize>,
    pub playing: bool,
    pub pos: f32,
    pub shuffle: bool,
    pub repeat: bool,
    /// 0.0 (mute) to 1.0.
    pub volume: f32,
}

pub struct App {
    pub lib: Library,
    /// The engine: the library database, the queue and the transport.
    pub backend: backend::Backend,
    pub adaptive: Adaptive<Layout>,
    pub dest: Dest,
    pub searching: bool,
    pub query: String,
    pub filter: Filter,
    /// Narrows the tag chips of the filter page as you type.
    pub tag_query: String,
    pub jam_open: bool,
    pub now_playing: bool,
    /// Full screen mode is on: the window is fullscreen, over Now Playing.
    pub fullscreen: bool,
    /// Whether Now Playing was open when full screen mode started, to go back to.
    pub np_before_fullscreen: bool,
    /// Whether the queue lists the tracks already played (collapsed until asked for).
    pub history_open: bool,
    /// Whether the queue lists what comes next (open until folded).
    pub up_next_open: bool,
    /// The volume before the volume button muted, to go back to.
    pub muted_from: Option<f32>,
    pub queue_open: bool,
    pub queue_slide: Slide,
    /// The window's height, for how far a sheet travels.
    pub win_h: f32,
    pub win_w: f32,
    /// Minimal mode: the window is the one-line player.
    pub minimal: bool,
    /// The window size to come back to when minimal mode ends.
    pub restore_size: Option<(f32, f32)>,
    /// Minimal mode shows the song, artist and controls instead of the spectrum.
    pub minimal_details: bool,
    /// The spectrum of what plays, as last read (drawn in minimal mode).
    pub spectrum: [f32; weave_player_lib::core::spectrum::BANDS],
    pub show_lyrics: bool,
    pub history: History<Place>,
    pub sort: SortState<Col>,
    /// The rows the current page lists, filtered and sorted.
    pub rows: Vec<Track>,
    pub player: Player,
    pub lyrics: Vec<Line>,
    lyrics_of: Option<usize>,
    pub lyric_line: Option<usize>,
    pub days: u8,
    /// The filter being edited in the playlist dialog, and its tag tree.
    pub draft: Option<Filter>,
    pub edit_track: Option<TrackEdit>,
    pub rate_track: Option<usize>,
    pub info_track: Option<usize>,
    /// One setting for every track list (and the jam's): rows without a picture, one line.
    /// A line shown for a moment (an import's result) and when it appeared.
    pub notice: Option<(String, Instant)>,
    pub sheet: Option<Sheet>,
    /// The order of Recent tracks.
    pub recent_order: RecentOrder,
    /// The engine's count of tracks started when the history was last read: when the session's
    /// differs, a track started and the history is read again.
    history_seq: u64,
    /// The user's density choice (the desktop's by default).
    pub density: DensityPref,
    /// "It's been a while" on Home: rated highly, not played for this many days.
    pub while_days: u32,
    pub favorites: std::collections::HashSet<String>,
    /// Seconds left on the sleep timer.
    pub sleep_left: Option<f32>,
    /// The minutes chosen in the sleep-timer menu (`None`: off).
    pub sleep_choice: Option<u32>,
    /// The window of the main track list (virtualised) and where it starts.
    pub list_vp: Viewport,
    /// Descriptions of detail pages, by `desc_key`, and whether the open one is being edited.
    pub descriptions: std::collections::HashMap<String, String>,
    /// The open Edit form, if any.
    pub edit: Option<DetailsDraft>,
    pub desc_expanded: bool,
    /// The jam sheet's matches as a virtualised list: the window of rows built, and where it
    /// starts in the full list.
    /// The jam's filters are open: a dialog wide, a full-screen sheet narrow. Closed by default;
    /// the jam page lists only the active filters.
    pub jam_filters_open: bool,
    /// The playlist the jam was last saved as, and the filters it was saved with.
    pub jam_saved: Option<(usize, Filter)>,
    pub jam_vp: Viewport,
    rng: u64,
}

pub const LYRICS: &str = "lyrics";

/// Seconds into a track after which Previous restarts it instead of going back.
const PREVIOUS_RESTARTS_AFTER: f32 = 3.0;

impl App {
    fn new() -> (Self, Task<Message>) {
        let (backend, lib) =
            backend::Backend::open(&backend::source_from_args()).unwrap_or_else(|e| {
                eprintln!("weave: cannot open the library: {e}");
                std::process::exit(1);
            });
        Self::with(backend, lib)
    }

    fn with(mut backend: backend::Backend, lib: Library) -> (Self, Task<Message>) {
        let dev = iad::dev::Switches::from_env();
        dev.apply();
        let session = backend.snapshot();
        let mut app = Self {
            backend,
            adaptive: Adaptive::new(
                Layouts::new()
                    .layout(..WidthClass::Md, Layout::Narrow)
                    .layout(WidthClass::Md.., Layout::Wide),
            )
            .overlap(true),
            dest: Dest::Home,
            searching: false,
            query: String::new(),
            filter: Filter::default(),
            tag_query: String::new(),
            jam_open: false,
            now_playing: false,
            fullscreen: false,
            np_before_fullscreen: false,
            history_open: false,
            up_next_open: true,
            muted_from: None,
            queue_open: false,
            queue_slide: Slide::default(),
            win_h: window_size().1,
            win_w: window_size().0,
            minimal: false,
            restore_size: None,
            minimal_details: false,
            spectrum: [0.0; weave_player_lib::core::spectrum::BANDS],
            show_lyrics: false,
            history: History::new(Place::home()),
            sort: SortState::new(),
            rows: Vec::new(),
            player: Player {
                queue: Vec::new(),
                up_next: editing_queue(Vec::new()),
                current: None,
                playing: false,
                pos: 0.0,
                shuffle: false,
                repeat: false,
                volume: 1.0,
            },
            lyrics: Vec::new(),
            lyrics_of: None,
            lyric_line: None,
            days: 7,
            draft: None,
            edit_track: None,
            rate_track: None,
            info_track: None,
            notice: None,
            sheet: None,
            recent_order: RecentOrder::Played,
            history_seq: 0,
            density: DensityPref::Desktop,
            while_days: 180,
            favorites: std::collections::HashSet::new(),
            sleep_left: None,
            sleep_choice: None,
            list_vp: Viewport::new(480.0),
            descriptions: std::collections::HashMap::new(),
            edit: None,
            desc_expanded: false,
            jam_filters_open: false,
            jam_saved: None,
            jam_vp: Viewport::new(JAM_LIST),
            rng: 0x9e37_79b9_7f4a_7c15,
            lib,
        };
        for (i, p) in app.lib.playlists.iter().enumerate() {
            app.descriptions
                .insert(format!("playlist:{i}"), p.description.clone());
        }
        app.apply(session);
        app.refresh();
        let cmds = app.adaptive.resize(window_size().0, Instant::now());
        let mut tasks = vec![app.dispatch(cmds)];
        if std::env::var("WEAVE_GUI_START").as_deref() == Ok("fullscreen") {
            // After the window is on screen, as when the full screen button is pressed.
            tasks.push(Task::future(async {
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(1500));
                    let _ = tx.send(());
                });
                let _ = rx.await;
                Message::Fullscreen(true)
            }));
        }
        let start_minimal = matches!(
            std::env::var("WEAVE_GUI_START").as_deref(),
            Ok("minimal" | "minimal-details")
        );
        if start_minimal {
            // After the window is on screen, as when the compact button is pressed.
            let delayed = |ms: u64, m: Message| {
                Task::future(async move {
                    let (tx, rx) = iced::futures::channel::oneshot::channel();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(ms));
                        let _ = tx.send(());
                    });
                    let _ = rx.await;
                    m
                })
            };
            tasks.push(delayed(1500, Message::Minimal(true)));
            if std::env::var("WEAVE_GUI_START").as_deref() == Ok("minimal-details") {
                tasks.push(delayed(2200, Message::MinimalDetails));
            }
        }
        if let Ok(start) = std::env::var("WEAVE_GUI_START") {
            for m in match start.as_str() {
                "jam" => vec![Message::ToggleJam],
                "jam-filters" => vec![Message::ToggleJam, Message::JamFilters(true)],
                "search" => vec![Message::SearchOpen(true), Message::SearchText("a".into())],
                "library" => vec![Message::Go(Dest::Tracks)],
                "compact-home" => vec![Message::SetDensity(DensityPref::Compact)],
                "compact-library" => vec![
                    Message::SetDensity(DensityPref::Compact),
                    Message::Go(Dest::Tracks),
                ],
                "track-sheet" => vec![
                    Message::Go(Dest::Tracks),
                    Message::OpenSheet(Some(Sheet::Track(3, true))),
                ],
                "shuffle-on" => vec![
                    Message::Go(Dest::Album(2)),
                    Message::SetShufflePref(Some(true)),
                ],
                "options-sheet" => vec![
                    Message::Go(Dest::Playlist(0)),
                    Message::OpenSheet(Some(Sheet::Options(true))),
                ],
                "home-end" => vec![
                    Message::Go(Dest::Home),
                    Message::ListScroll(Viewport::new(480.0)),
                ],
                "explore" => vec![Message::Go(Dest::Explore)],
                "albums" => vec![Message::Go(Dest::Albums)],
                "playing-queue" => vec![Message::NowPlaying(true), Message::ToggleQueue],
                "artists" => vec![Message::Go(Dest::Artists)],
                "playlists" => vec![Message::Go(Dest::Playlists)],
                "recent" => vec![Message::Go(Dest::Recent)],
                "popover" => vec![Message::SearchOpen(true)],
                "playlist" => vec![Message::Go(Dest::Playlist(0))],
                "playlist-in" => vec![Message::Open(Dest::Playlist(0))],
                "edit-track" => vec![Message::Go(Dest::Tracks), Message::EditTrack(3)],
                "describe" => vec![Message::Go(Dest::Artist(1)), Message::EditDetails],
                "edit-playlist" => vec![Message::Go(Dest::Playlist(0)), Message::EditDetails],
                "long-desc" => vec![
                    Message::Go(Dest::Playlist(0)),
                    Message::Describe(LONG.repeat(3)),
                    Message::DescExpanded(std::env::var_os("WEAVE_GUI_EXPANDED").is_some()),
                ],
                "dialog" => vec![Message::Go(Dest::Playlist(0)), Message::EditFilter],
                "artist" => vec![Message::Go(Dest::Artist(1))],
                "album" => vec![Message::Go(Dest::Album(2))],
                "playing" => vec![Message::NowPlaying(true)],
                "queue" => vec![Message::ToggleQueue],
                "lyrics" => vec![
                    Message::NowPlaying(true),
                    Message::ToggleLyrics,
                    Message::Seek(40.0),
                ],
                _ => vec![],
            } {
                tasks.push(Task::done(m));
            }
        }
        // Development switches (screenshot, holding animations) are iad's, from the environment.
        if dev.screenshot.is_some() {
            // Minimal mode opens a window of its own, so the shot is of the newest one, taken
            // after that has happened.
            let window = if start_minimal {
                Task::future(async {
                    let (tx, rx) = iced::futures::channel::oneshot::channel();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(3000));
                        let _ = tx.send(());
                    });
                    let _ = rx.await;
                })
                .then(|_| iced::window::latest())
            } else {
                iced::window::oldest()
            };
            tasks.push(
                window
                    .and_then(move |id| dev.capture(id).unwrap_or_else(Task::none))
                    .map(Message::Captured),
            );
        }
        tasks.push(iad::system::detect_color_scheme().map(Message::SystemScheme));
        (app, Task::batch(tasks))
    }

    /// Layouts here answer their transition commands at once.
    fn dispatch(&mut self, cmds: Vec<iad::transition::Command<Layout>>) -> Task<Message> {
        Task::batch(
            cmds.into_iter()
                .map(|c| Task::done(Message::Transition(c.done()))),
        )
    }

    fn close_overlays(&mut self) {
        self.draft = None;
        self.edit_track = None;
        self.edit = None;
        self.desc_expanded = false;
        self.jam_open = false;
        self.now_playing = false;
        self.queue_open = false;
    }

    /// Shows a page: the current search ends and anything open over the page closes.
    fn show(&mut self, d: Dest) -> Vec<iad::transition::Command<Layout>> {
        self.dest = d;
        self.searching = false;
        self.close_overlays();
        self.refresh();
        Vec::new()
    }

    /// What a detail page's description is stored under.
    pub fn desc_key(&self) -> Option<String> {
        match self.dest {
            Dest::Playlist(i) => Some(format!("playlist:{i}")),
            Dest::Artist(i) => self.lib.artists().get(i).map(|a| format!("artist:{}", a.0)),
            Dest::Album(i) => self
                .lib
                .albums()
                .get(i)
                .map(|a| format!("album:{}|{}", a.0, a.1)),
            _ => None,
        }
    }

    /// Puts a track in the queue: right after the one playing, or at the end. With nothing
    /// queued it starts playing.
    fn enqueue(&mut self, id: usize, next: bool) {
        let session = self.backend.enqueue(id, next);
        self.apply(session);
    }

    /// Reads the listening history again and puts it on the library; Recent tracks follows it.
    fn refresh_history(&mut self) {
        let stats = self.backend.history();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        self.lib.apply_history(&stats, now);
        if self.dest == Dest::Recent {
            self.refresh();
        }
    }

    /// Shows a line for a few seconds.
    fn say(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Reads the library again: the lists, the playlists' descriptions, and the queue by slug,
    /// since the indexes shift when tracks or playlists are added or renamed. `false` when the
    /// library cannot be read (the reason is shown).
    fn reload_library(&mut self) -> bool {
        match self.backend.load() {
            Ok(lib) => {
                self.lib = lib;
                for (i, p) in self.lib.playlists.iter().enumerate() {
                    self.descriptions
                        .insert(format!("playlist:{i}"), p.description.clone());
                }
                let s = self.backend.snapshot();
                self.apply(s);
                self.refresh();
                true
            }
            Err(e) => {
                self.say(format!("Cannot read the library: {e}"));
                false
            }
        }
    }

    /// After an import: reads the library again and leaves any page addressed by an index, which
    /// may now mean something else.
    fn reload(&mut self) {
        if self.reload_library()
            && matches!(
                self.dest,
                Dest::Artist(_) | Dest::Album(_) | Dest::Playlist(_)
            )
        {
            self.dest = Dest::Home;
            self.history = History::new(Place::home());
            self.refresh();
        }
    }

    /// Opens the Edit form of the page: a playlist's name and the description, as they are now.
    fn open_form(&mut self) {
        let Some(key) = self.desc_key() else { return };
        // The default playlist keeps its name.
        let name = match self.dest {
            Dest::Playlist(i) => self
                .lib
                .playlists
                .get(i)
                .map(|p| p.name.clone())
                .filter(|n| n != backend::FAVOURITES),
            _ => None,
        };
        self.edit = Some(DetailsDraft {
            name,
            description: self.descriptions.get(&key).cloned().unwrap_or_default(),
        });
    }

    /// Saves the form. A playlist's description and name go to the engine; the name is checked
    /// there (not empty, not another playlist's), and a refusal is shown with the form closed and
    /// the name unchanged. An artist's or an album's description is kept in the app.
    fn save_form(&mut self) {
        let Some(draft) = self.edit.take() else {
            return;
        };
        let Some(key) = self.desc_key() else { return };
        let Dest::Playlist(i) = self.dest else {
            self.descriptions.insert(key, draft.description);
            return;
        };
        let Some(old) = self.lib.playlists.get(i).map(|p| p.name.clone()) else {
            return;
        };
        match self
            .backend
            .set_playlist_description(&old, &draft.description)
        {
            Ok(()) => {
                self.lib.playlists[i].description = draft.description.clone();
                self.descriptions.insert(key, draft.description);
            }
            Err(e) => self.say(format!("Cannot save the description: {e}")),
        }
        let Some(new) = draft
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| *n != old)
        else {
            return;
        };
        match self.backend.rename_playlist(&old, &new) {
            Ok(()) => {
                if self.reload_library() {
                    // The list is sorted by name, so the playlist may have moved.
                    if let Some(at) = self.lib.playlists.iter().position(|p| p.name == new) {
                        self.dest = Dest::Playlist(at);
                        self.history = History::new(Place::home());
                        self.refresh();
                    }
                }
            }
            Err(e) => self.say(format!("Cannot rename: {e}")),
        }
    }

    /// Mirrors the engine's session into the UI's player state.
    fn apply(&mut self, s: backend::Snapshot) {
        // A track started since the history was read: it is a play now.
        if s.started != self.history_seq {
            self.history_seq = s.started;
            self.refresh_history();
        }
        self.player.queue = s.queue;
        self.player.current = s.current;
        self.player.playing = s.playing;
        self.player.pos = s.pos;
        self.player.shuffle = s.shuffle;
        self.player.repeat = s.repeat;
        self.player.volume = s.volume;
        self.player.sync_up_next();
    }

    /// Whether the track is in the Favourites playlist.
    pub fn is_favourite(&self, id: usize) -> bool {
        self.lib
            .playlists
            .iter()
            .any(|p| p.name == backend::FAVOURITES && p.filter.is_none() && p.tracks.contains(&id))
    }

    pub fn track(&self, id: usize) -> &Track {
        &self.lib.tracks[id]
    }

    /// Whether Next does anything: there is a track after the playing one, or the queue repeats.
    pub fn can_next(&self) -> bool {
        self.player.current.is_some_and(|c| {
            c + 1 < self.player.queue.len() || (self.player.repeat && !self.player.queue.is_empty())
        })
    }

    /// Whether Previous does anything: there is a track before the playing one, the queue
    /// repeats, or the track is far enough in for Previous to restart it.
    pub fn can_previous(&self) -> bool {
        self.player.current.is_some_and(|c| {
            c > 0 || self.player.repeat || self.player.pos > PREVIOUS_RESTARTS_AFTER
        })
    }

    /// The track playing now.
    pub fn now(&self) -> Option<&Track> {
        let i = self.player.current?;
        self.player.queue.get(i).map(|id| self.track(*id))
    }

    /// Rebuilds the rows of the current page: its scope, then the search, then the sort.
    pub fn refresh(&mut self) {
        let mut rows: Vec<Track> = if self.searching {
            let q = self.query.trim().to_lowercase();
            self.lib
                .filtered(&self.filter)
                .into_iter()
                .filter(|t| {
                    q.is_empty()
                        || [&t.title, &t.artist, &t.album]
                            .iter()
                            .any(|f| f.to_lowercase().contains(&q))
                })
                .cloned()
                .collect()
        } else {
            match self.dest {
                // Recent tracks: what was played, or until anything was, what was added; or by when
                // they were added.
                Dest::Recent => self
                    .lib
                    .recent_tracks(30, self.recent_order)
                    .into_iter()
                    .cloned()
                    .collect(),
                Dest::Artist(i) => {
                    let name = self
                        .lib
                        .artists()
                        .get(i)
                        .map(|a| a.0.clone())
                        .unwrap_or_default();
                    self.lib
                        .tracks
                        .iter()
                        .filter(|t| t.artist == name)
                        .cloned()
                        .collect()
                }
                Dest::Album(i) => {
                    let (album, artist) = self
                        .lib
                        .albums()
                        .get(i)
                        .map(|a| (a.0.clone(), a.1.clone()))
                        .unwrap_or_default();
                    self.lib
                        .tracks
                        .iter()
                        .filter(|t| t.album == album && t.artist == artist)
                        .cloned()
                        .collect()
                }
                Dest::Playlist(i) => self
                    .lib
                    .playlists
                    .get(i)
                    .map(|p| self.lib.playlist_tracks(p).into_iter().cloned().collect())
                    .unwrap_or_default(),
                _ => self.lib.tracks.clone(),
            }
        };
        self.sort.apply(&mut rows, |col, a, b| match col {
            Col::Title => cmp_str(&a.title.to_lowercase(), &b.title.to_lowercase()),
            Col::Artist => cmp_str(&a.artist.to_lowercase(), &b.artist.to_lowercase()),
            Col::Album => cmp_str(&a.album.to_lowercase(), &b.album.to_lowercase()),
            Col::Duration => a.secs.cmp(&b.secs),
        });
        self.rows = rows;
    }

    fn roll(&mut self) -> u64 {
        // xorshift64: enough for "Surprise me" and shuffling.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Plays a track picked in a list. Nothing is cleared: with a queue, the highlight moves to the
    /// track (queued first if it was not there). Only when the queue is empty does the list
    /// become the queue, so Next has somewhere to go.
    fn play_from_list(&mut self, ids: Vec<usize>, start: usize, id: usize) {
        if self.player.queue.is_empty() {
            self.apply_page_shuffle();
            self.play(ids, start);
        } else {
            let s = self.backend.play_in_queue(id);
            self.apply(s);
        }
    }

    /// Whether the open playlist or album plays shuffled: `None` for a page that has no such
    /// setting (an artist), else its shuffle toggle (off until it is switched on).
    pub fn page_shuffle(&self) -> Option<bool> {
        match self.dest {
            Dest::Playlist(i) => self
                .lib
                .playlists
                .get(i)
                .map(|p| p.shuffle.unwrap_or(false)),
            Dest::Album(i) => self.lib.albums().get(i).map(|(album, artist, _)| {
                self.lib
                    .album_shuffle
                    .get(&(album.clone(), artist.clone()))
                    .copied()
                    .unwrap_or(false)
            }),
            _ => None,
        }
    }

    /// Before the open page's list is played: the player's shuffle is set as the page's toggle
    /// says (a page without one leaves it). Returns whether the page plays shuffled.
    fn apply_page_shuffle(&mut self) -> bool {
        match self.page_shuffle() {
            Some(on) => {
                let s = self.backend.shuffle(on);
                self.apply(s);
                on
            }
            None => false,
        }
    }

    fn play(&mut self, ids: Vec<usize>, start: usize) {
        let session = self.backend.play(&ids, start);
        self.apply(session);
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let now = Instant::now();
        let cmds = match message {
            Message::Resized(w, h) => {
                self.win_h = h;
                self.win_w = w;
                self.adaptive.resize(w, now)
            }
            Message::Frame(t) => {
                self.queue_slide.drive(self.queue_open, t);
                Vec::new()
            }
            Message::Tick(t) => self.adaptive.tick(t),
            Message::Transition(e) => self.adaptive.report(e, now),
            Message::Minimal(on) => {
                if on == self.minimal {
                    return Task::none();
                }
                self.minimal = on;
                self.minimal_details = false;
                let settings = if on {
                    // Overlays go; the player stays.
                    self.now_playing = false;
                    self.queue_open = false;
                    self.restore_size = Some((self.win_w, self.win_h));
                    iced::window::Settings {
                        size: iced::Size::new(MINIMAL.0, MINIMAL.1),
                        min_size: Some(iced::Size::new(MINIMAL_MIN_WIDTH, MINIMAL.1)),
                        // As high as the strip, and as wide as the user pulls it.
                        max_size: Some(iced::Size::new(MINIMAL_MAX_WIDTH, MINIMAL.1)),
                        ..iad::windows::settings_for(platforms::APP_ID)
                    }
                } else {
                    let (w, h) = self.restore_size.take().unwrap_or_else(window_size);
                    iced::window::Settings {
                        size: iced::Size::new(w, h),
                        ..iad::windows::settings_for(platforms::APP_ID)
                    }
                };
                // A new window of the size wanted, then the old one closes: on Wayland a window
                // cannot be told its new size (winit sends no resize event for a request, so the
                // app would keep drawing at the old size).
                return iced::window::oldest().and_then(move |old| {
                    let (_, open) = iced::window::open(settings.clone());
                    open.then(move |_| iced::window::close(old))
                });
            }
            Message::MinimalDetails => {
                self.minimal_details = !self.minimal_details;
                Vec::new()
            }
            Message::SpectrumTick => {
                self.spectrum = weave_player_lib::core::spectrum::bands();
                Vec::new()
            }
            Message::Window(a) => {
                return iced::window::oldest().and_then(move |id| a.perform(id));
            }
            Message::Go(d) => {
                self.history.go(Place::at(d));
                self.show(d)
            }
            Message::Open(d) => {
                // The place being left may have changed since it was entered (a search typed).
                let here = Place {
                    dest: self.dest,
                    searching: self.searching,
                    query: self.query.clone(),
                };
                self.history.replace(here);
                self.history.open(Place::at(d));
                self.show(d)
            }
            Message::SearchOpen(false) if !self.is_narrow() && self.searching => {
                return self.update(Message::Go(Dest::Home));
            }
            Message::SearchOpen(on) => {
                self.searching = on;
                self.jam_open = false;
                self.now_playing = false;
                if !on {
                    self.query.clear();
                    self.filter = Filter::default();
                }
                self.refresh();
                Vec::new()
            }
            Message::SearchText(t) => {
                self.query = t;
                self.refresh();
                Vec::new()
            }
            Message::ToggleJam => {
                if self.jam_open {
                    // On desktop, unselecting a header button returns to Home.
                    if !self.is_narrow() {
                        return self.update(Message::Go(Dest::Home));
                    }
                    self.jam_open = false;
                } else {
                    self.jam_open = true;
                    self.now_playing = false;
                    self.searching = false;
                }
                Vec::new()
            }
            Message::ToggleExplore => {
                if self.dest == Dest::Explore && !self.jam_open && !self.searching {
                    return self.update(Message::Go(Dest::Home));
                }
                return self.update(Message::Go(Dest::Explore));
            }
            Message::SetDensity(d) => {
                self.density = d;
                Vec::new()
            }
            Message::SetShufflePref(pref) => {
                match self.dest {
                    Dest::Playlist(i) => {
                        if let Some(name) = self.lib.playlists.get(i).map(|p| p.name.clone()) {
                            match self.backend.set_playlist_shuffle(&name, pref) {
                                Ok(()) => self.lib.playlists[i].shuffle = pref,
                                Err(e) => self.say(format!("Cannot save the setting: {e}")),
                            }
                        }
                    }
                    Dest::Album(i) => {
                        let key = self
                            .lib
                            .albums()
                            .get(i)
                            .map(|(album, artist, _)| (album.clone(), artist.clone()));
                        let slug = key.as_ref().and_then(|(album, artist)| {
                            self.lib
                                .tracks
                                .iter()
                                .find(|t| t.album == *album && t.artist == *artist)
                                .map(|t| t.slug.clone())
                        });
                        if let (Some(key), Some(slug)) = (key, slug) {
                            match self.backend.set_album_shuffle(&slug, pref) {
                                Ok(()) => match pref {
                                    Some(on) => {
                                        self.lib.album_shuffle.insert(key, on);
                                    }
                                    None => {
                                        self.lib.album_shuffle.remove(&key);
                                    }
                                },
                                Err(e) => self.say(format!("Cannot save the setting: {e}")),
                            }
                        }
                    }
                    _ => {}
                }
                Vec::new()
            }
            Message::SetRecentOrder(order) => {
                self.recent_order = order;
                self.refresh();
                Vec::new()
            }
            Message::OpenSheet(s) => {
                self.sheet = s;
                Vec::new()
            }
            Message::SheetAction(m) => {
                self.sheet = None;
                return self.update(*m);
            }
            Message::Import(files) => {
                return Task::perform(pick(files), Message::Picked);
            }
            Message::Picked(paths) => {
                let Some(db) = self.backend.db().map(|d| d.to_path_buf()) else {
                    return Task::none();
                };
                if paths.is_empty() {
                    return Task::none();
                }
                self.say("Importing…");
                return Task::perform(import(db, paths), Message::Imported);
            }
            Message::Imported(result) => {
                match result {
                    Ok(text) => {
                        self.reload();
                        self.say(text);
                    }
                    Err(e) => self.say(format!("Import failed: {e}")),
                }
                Vec::new()
            }
            Message::WhileDays(d) => {
                self.while_days = d;
                Vec::new()
            }
            Message::ToggleFavoriteTrack(id) => {
                let on = !self.is_favourite(id);
                let slug = self.lib.tracks[id].slug.clone();
                match self.backend.set_favourite(&slug, on) {
                    Ok(()) => {
                        match self
                            .lib
                            .playlists
                            .iter()
                            .position(|p| p.name == backend::FAVOURITES)
                        {
                            Some(i) => {
                                let tracks = &mut self.lib.playlists[i].tracks;
                                if on {
                                    tracks.push(id);
                                } else {
                                    tracks.retain(|t| *t != id);
                                }
                                self.refresh();
                            }
                            // The playlist did not exist when the library was read.
                            None => {
                                self.reload_library();
                            }
                        }
                    }
                    Err(e) => self.say(format!("Cannot change the favourites: {e}")),
                }
                Vec::new()
            }
            Message::ToggleFavorite(a) => {
                if !self.favorites.remove(&a) {
                    self.favorites.insert(a);
                }
                Vec::new()
            }
            Message::SleepTimer(m) => {
                self.sleep_left = m.map(|m| m as f32 * 60.0);
                self.sleep_choice = m;
                Vec::new()
            }
            Message::ListScroll(v) => {
                self.list_vp = v;
                Vec::new()
            }
            Message::RateTrack(id) => {
                self.rate_track = Some(id);
                Vec::new()
            }
            Message::SetRating(id, r) => {
                if let Some(t) = self.lib.tracks.get_mut(id) {
                    // Pressing the current rating clears it.
                    t.rating = if t.rating == r { 0 } else { r };
                }
                self.rate_track = None;
                self.refresh();
                Vec::new()
            }
            Message::CloseRate => {
                self.rate_track = None;
                Vec::new()
            }
            Message::InfoTrack(id) => {
                self.info_track = Some(id);
                Vec::new()
            }
            Message::CloseInfo => {
                self.info_track = None;
                Vec::new()
            }
            Message::ClearQueue => {
                let s = self.backend.clear_up_next();
                self.apply(s);
                Vec::new()
            }
            Message::QueueJump(at) => {
                let s = self.backend.jump(at);
                self.apply(s);
                Vec::new()
            }
            Message::ToggleUpNext => {
                self.up_next_open = !self.up_next_open;
                Vec::new()
            }
            Message::ToggleQueueHistory => {
                self.history_open = !self.history_open;
                Vec::new()
            }
            Message::QueueRemove(i) => {
                // `i` counts in the "Up next" list, which starts after the playing track.
                let at = self.player.current.map_or(0, |c| c + 1) + i;
                if at < self.player.queue.len() {
                    let s = self.backend.remove(at);
                    self.apply(s);
                }
                Vec::new()
            }
            Message::NowPlaying(false) if self.fullscreen => {
                // Closing Now Playing ends full screen mode too.
                self.np_before_fullscreen = false;
                return self.update(Message::Fullscreen(false));
            }
            Message::Fullscreen(on) => {
                if on == self.fullscreen {
                    return Task::none();
                }
                self.fullscreen = on;
                if on {
                    self.np_before_fullscreen = self.now_playing;
                    self.now_playing = true;
                    self.queue_open = false;
                } else {
                    self.now_playing = self.np_before_fullscreen;
                }
                let mode = if on {
                    iced::window::Mode::Fullscreen
                } else {
                    iced::window::Mode::Windowed
                };
                return iced::window::oldest().and_then(move |id| iced::window::set_mode(id, mode));
            }
            Message::ToggleFullscreen => {
                return self.update(Message::Fullscreen(!self.fullscreen));
            }
            Message::EscapeKey => {
                if self.fullscreen {
                    return self.update(Message::Fullscreen(false));
                }
                return Task::none();
            }
            Message::NowPlaying(on) => {
                self.now_playing = on;
                if on {
                    // Full-screen Now Playing and the queue page never show together.
                    self.queue_open = false;
                }
                Vec::new()
            }
            Message::ToggleQueue => {
                self.queue_open = !self.queue_open;
                if self.queue_open {
                    // Opening the queue collapses Now Playing, so the queue is what you see.
                    self.now_playing = false;
                }
                Vec::new()
            }
            Message::ToggleLyrics => {
                self.show_lyrics = !self.show_lyrics;
                Vec::new()
            }
            Message::ToggleTag(t) => {
                toggle(&mut self.filter.tags, t);
                self.refresh();
                Vec::new()
            }
            Message::TagQuery(q) => {
                self.tag_query = q;
                Vec::new()
            }
            Message::Bpm(r) => {
                self.filter.bpm = r;
                self.refresh();
                Vec::new()
            }
            Message::ClearFilter => {
                self.filter = Filter::default();
                self.refresh();
                Vec::new()
            }
            Message::Surprise => {
                // Replaces the filter; never a combination that matches nothing.
                let names = self.lib.tag_names();
                for _ in 0..30 {
                    let mut f = Filter::default();
                    let picks = 1 + (self.roll() % 2) as usize;
                    for _ in 0..picks {
                        let t = names[(self.roll() % names.len() as u64) as usize].clone();
                        if !f.tags.contains(&t) {
                            f.tags.push(t);
                        }
                    }
                    if self.lib.filtered(&f).len() >= 3 {
                        self.filter = f;
                        break;
                    }
                }
                self.refresh();
                Vec::new()
            }
            Message::StartJam => {
                let mut ids: Vec<usize> = self
                    .lib
                    .filtered(&self.filter)
                    .iter()
                    .map(|t| t.id)
                    .collect();
                for i in (1..ids.len()).rev() {
                    let j = (self.roll() % (i as u64 + 1)) as usize;
                    ids.swap(i, j);
                }
                if !ids.is_empty() {
                    self.play(ids, 0);
                    self.player.shuffle = true;
                }
                self.jam_open = false;
                Vec::new()
            }
            Message::Sort(c) => {
                self.sort.activate(c, false);
                self.refresh();
                Vec::new()
            }
            Message::PlayTrack(id) => {
                let ids: Vec<usize> = self.rows.iter().map(|t| t.id).collect();
                let start = ids.iter().position(|i| *i == id).unwrap_or(0);
                self.play_from_list(ids, start, id);
                Vec::new()
            }
            Message::PlayOne(id) => {
                self.play_from_list(vec![id], 0, id);
                Vec::new()
            }
            Message::PlayAll(ids) => {
                if !ids.is_empty() {
                    // A page set to shuffle opens on a random track, as shuffling a playlist does.
                    let start = if self.apply_page_shuffle() && ids.len() > 1 {
                        (self.roll() % ids.len() as u64) as usize
                    } else {
                        0
                    };
                    self.play(ids, start);
                }
                Vec::new()
            }
            Message::SaveJamAsPlaylist => {
                let n = self.lib.filtered(&self.filter).len();
                if n > 0 {
                    let name = self.new_playlist_name();
                    self.lib.playlists.push(Playlist {
                        name: name.clone(),
                        icon: None,
                        description: String::new(),
                        filter: Some(self.filter.clone()),
                        tracks: Vec::new(),
                        shuffle: None,
                    });
                    let i = self.lib.playlists.len() - 1;
                    self.descriptions
                        .insert(format!("playlist:{i}"), String::new());
                    self.jam_saved = Some((i, self.filter.clone()));
                }
                Vec::new()
            }
            Message::JamFilters(on) => {
                self.jam_filters_open = on;
                Vec::new()
            }
            Message::PlayNext(id) => {
                self.enqueue(id, true);
                Vec::new()
            }
            Message::AddToQueue(id) => {
                self.enqueue(id, false);
                Vec::new()
            }
            Message::ViewAlbum(id) => {
                let t = self.track(id);
                let at = self
                    .lib
                    .albums()
                    .iter()
                    .position(|a| a.0 == t.album && a.1 == t.artist);
                if let Some(i) = at {
                    return self.update(Message::Open(Dest::Album(i)));
                }
                Vec::new()
            }
            Message::ViewArtist(id) => {
                let name = self.track(id).artist.clone();
                let at = self.lib.artists().iter().position(|a| a.0 == name);
                if let Some(i) = at {
                    return self.update(Message::Open(Dest::Artist(i)));
                }
                Vec::new()
            }
            Message::EditTrack(id) => {
                let t = self.track(id);
                self.edit_track = Some(TrackEdit {
                    id,
                    tags: t.tags.clone(),
                });
                Vec::new()
            }
            Message::EditToggleTag(t) => {
                if let Some(e) = &mut self.edit_track {
                    toggle(&mut e.tags, t);
                }
                Vec::new()
            }
            Message::SaveTrack => {
                if let Some(e) = self.edit_track.take()
                    && let Some(t) = self.lib.tracks.get_mut(e.id)
                {
                    t.tags = e.tags;
                }
                self.refresh();
                Vec::new()
            }
            Message::CancelTrack => {
                self.edit_track = None;
                Vec::new()
            }
            Message::JamScroll(v) => {
                self.jam_vp = v;
                Vec::new()
            }
            Message::Back => {
                if let Some(p) = self.history.back().cloned() {
                    self.dest = p.dest;
                    self.searching = p.searching;
                    self.query = p.query;
                    self.close_overlays();
                    self.refresh();
                }
                Vec::new()
            }
            Message::Describe(t) => {
                if let Some(k) = self.desc_key() {
                    self.descriptions.insert(k, t);
                }
                Vec::new()
            }
            Message::DescExpanded(on) => {
                self.desc_expanded = on;
                Vec::new()
            }
            Message::EditDetails => {
                self.open_form();
                Vec::new()
            }
            Message::SaveDetails => {
                self.save_form();
                Vec::new()
            }
            Message::CancelDetails => {
                self.edit = None;
                Vec::new()
            }
            Message::NameInput(t) => {
                if let Some(DetailsDraft { name: Some(n), .. }) = &mut self.edit {
                    *n = t;
                }
                Vec::new()
            }
            Message::DescInput(t) => {
                if let Some(d) = &mut self.edit {
                    d.description = t;
                }
                Vec::new()
            }
            Message::EditFilter => {
                if let Dest::Playlist(i) = self.dest {
                    self.draft = self.lib.playlists.get(i).and_then(|p| p.filter.clone());
                }
                Vec::new()
            }
            Message::DraftToggle(t) => {
                if let Some(d) = &mut self.draft {
                    toggle(&mut d.tags, t);
                }
                Vec::new()
            }
            Message::DraftBpm(r) => {
                if let Some(d) = &mut self.draft {
                    d.bpm = r;
                }
                Vec::new()
            }
            Message::SaveFilter => {
                if let (Dest::Playlist(i), Some(f)) = (self.dest, self.draft.take())
                    && let Some(p) = self.lib.playlists.get_mut(i)
                {
                    p.filter = Some(f);
                }
                self.refresh();
                Vec::new()
            }
            Message::CancelFilter => {
                self.draft = None;
                Vec::new()
            }
            Message::Days(d) => {
                self.days = d;
                Vec::new()
            }
            Message::TogglePlay => {
                let s = self.backend.toggle(self.player.playing);
                self.apply(s);
                Vec::new()
            }
            Message::Next => {
                let s = self.backend.next();
                self.apply(s);
                Vec::new()
            }
            Message::Previous => {
                // Past the first seconds, "previous" restarts the track.
                let s = if self.player.pos > PREVIOUS_RESTARTS_AFTER {
                    self.backend.seek(0.0)
                } else {
                    self.backend.previous()
                };
                self.apply(s);
                Vec::new()
            }
            Message::Seek(p) => {
                let s = self.backend.seek(p);
                self.apply(s);
                Vec::new()
            }
            Message::Shuffle => {
                let s = self.backend.shuffle(!self.player.shuffle);
                self.apply(s);
                Vec::new()
            }
            Message::SetVolume(v) => {
                self.muted_from = None;
                let s = self.backend.set_volume(v);
                self.apply(s);
                Vec::new()
            }
            Message::ToggleMute => {
                let target = if self.player.volume == 0.0 {
                    // Muted: back to the volume before, or half when there is none.
                    self.muted_from.take().filter(|v| *v > 0.0).unwrap_or(0.5)
                } else {
                    self.muted_from = Some(self.player.volume);
                    0.0
                };
                let s = self.backend.set_volume(target);
                self.apply(s);
                Vec::new()
            }
            Message::Repeat => {
                let s = self.backend.repeat(!self.player.repeat);
                self.apply(s);
                Vec::new()
            }
            Message::Queue(e) => {
                // A live list: a drop is committed at once, in the queue after the playing
                // track. `from` and `to` count in "Up next".
                if let ReorderEvent::Reorder { from, to } = e {
                    let base = self.player.current.map_or(0, |c| c + 1);
                    let s = self.backend.move_item(base + from, base + to);
                    self.apply(s);
                } else {
                    self.player.up_next.apply(e);
                }
                Vec::new()
            }
            Message::Captured(_) => return iced::exit(),
            Message::Progress => {
                if let Some(left) = &mut self.sleep_left {
                    *left -= 0.25;
                    if *left <= 0.0 {
                        self.sleep_left = None;
                        self.sleep_choice = None;
                        let s = self.backend.pause();
                        self.apply(s);
                    }
                }
                Vec::new()
            }
            Message::SystemScheme(_) => Vec::new(),
            Message::Poll => {
                // The session as it is now: the position, and what another handle changed.
                let s = self.backend.snapshot();
                self.apply(s);
                if self
                    .notice
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(4))
                {
                    self.notice = None;
                }
                Vec::new()
            }
        };
        let snap = self.sync_lyrics();
        Task::batch([self.dispatch(cmds), snap])
    }

    /// A name for a playlist saved from the jam: its tags and tempo, made unique.
    fn new_playlist_name(&self) -> String {
        let mut parts: Vec<String> = self
            .filter
            .tags
            .iter()
            .map(|t| t.split_once(':').map_or(t.as_str(), |(_, v)| v).to_string())
            .collect();
        if self.filter.bpm != data::BPM {
            parts.push(format!(
                "{:.0}–{:.0} BPM",
                self.filter.bpm.0, self.filter.bpm.1
            ));
        }
        let base = if parts.is_empty() {
            "Jam".to_string()
        } else {
            format!("Jam: {}", parts.join(" · "))
        };
        let mut name = base.clone();
        let mut n = 2;
        while self
            .lib
            .playlists
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&name))
        {
            name = format!("{base} {n}");
            n += 1;
        }
        name
    }

    /// Whether a sheet or the queue is still sliding.
    pub fn animating(&self) -> bool {
        !self.queue_slide.is_open_at(self.queue_open)
    }

    /// Compact lists: the user's choice, or else the desktop's density.
    pub fn compact(&self) -> bool {
        match self.density {
            DensityPref::Compact => true,
            DensityPref::Default => false,
            DensityPref::Desktop => iad::config::current().density == iad::config::Density::Compact,
        }
    }

    /// The height of a row in every track list.
    pub fn row_h(&self) -> f32 {
        if self.compact() { ROW_COMPACT } else { ROW }
    }

    /// Keeps the lyrics of the playing track and the line to highlight; scrolls when it moves.
    fn sync_lyrics(&mut self) -> Task<Message> {
        let id = self.now().map(|t| t.id);
        if id != self.lyrics_of {
            self.lyrics_of = id;
            self.lyrics = id
                .map(|i| fake_lyrics(&self.track(i).title, self.track(i).secs))
                .unwrap_or_default();
            self.lyric_line = None;
        }
        let line = synced::current(&self.lyrics, self.player.pos);
        if line == self.lyric_line {
            return Task::none();
        }
        self.lyric_line = line;
        if self.now_playing && self.show_lyrics {
            iad::prelude::follow_synced_text(LYRICS, line, self.lyrics.len())
        } else {
            Task::none()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            iced::window::resize_events().map(|(_, s)| Message::Resized(s.width, s.height)),
            self.adaptive.subscription(Message::Tick),
            iad::system::follow_color_scheme().map(Message::SystemScheme),
        ];
        subs.push(iced::keyboard::listen().filter_map(|event| {
            use iced::keyboard::{Event, Key, key::Named};
            match event {
                Event::KeyPressed {
                    key: Key::Named(Named::F11),
                    ..
                } => Some(Message::ToggleFullscreen),
                Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                } => Some(Message::EscapeKey),
                _ => None,
            }
        }));
        if self.animating() {
            subs.push(iced::window::frames().map(Message::Frame));
        }
        // The position moves while playing; otherwise the session is only checked now and then.
        let poll = if self.player.playing { 250 } else { 1000 };
        subs.push(iced::time::every(Duration::from_millis(poll)).map(|_| Message::Poll));
        if self.player.playing {
            subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::Progress));
        }
        // The bars are read as often as they are drawn, and until they have fallen quiet.
        if self.minimal && (self.player.playing || self.spectrum.iter().any(|l| *l > 0.0)) {
            subs.push(iced::time::every(Duration::from_millis(33)).map(|_| Message::SpectrumTick));
        }
        Subscription::batch(subs)
    }

    fn title(&self) -> String {
        match self.now() {
            Some(t) if self.player.playing => format!("{} – {}", t.title, t.artist),
            _ => "Weave".into(),
        }
    }

    fn theme(&self) -> iad::Theme {
        iad::Theme::from_config(&iad::config::current())
    }
}

/// A queue that is always editable: a live list commits every drop at once.
fn editing_queue(items: Vec<usize>) -> ReorderState<usize> {
    ReorderState::live(items)
}

/// Adds `tag` to the list, or removes it when it is there.
fn toggle(tags: &mut Vec<String>, tag: String) {
    match tags.iter().position(|t| *t == tag) {
        Some(i) => {
            tags.remove(i);
        }
        None => tags.push(tag),
    }
}

/// A long description to judge the clamp with (`WEAVE_GUI_START=long-desc`).
const LONG: &str = "A long description that goes on about the music in this list: where it came from, why it is here, and what it is good for on a long drive. ";

/// The system file chooser: files, or a folder. Empty when cancelled.
async fn pick(files: bool) -> Vec<std::path::PathBuf> {
    let dialog = rfd::AsyncFileDialog::new().set_title(if files {
        "Import files"
    } else {
        "Import a folder"
    });
    let picked = if files {
        dialog
            .add_filter(
                "Audio",
                &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav"],
            )
            .pick_files()
            .await
    } else {
        dialog.pick_folders().await
    };
    picked
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.path().to_path_buf())
        .collect()
}

/// Indexes `paths` on a thread of its own, so the window keeps drawing.
async fn import(db: std::path::PathBuf, paths: Vec<std::path::PathBuf>) -> Result<String, String> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(backend::import(&db, &paths));
    });
    rx.await
        .unwrap_or_else(|_| Err("the import stopped".into()))
}

fn fake_lyrics(title: &str, secs: u32) -> Vec<Line> {
    const LINES: [&str; 8] = [
        "Dime que sí, dime que vuelves",
        "Bajo la luna, bailamos otra vez",
        "No hay nadie más, solo tú y yo",
        "Cada latido me trae tu nombre",
        "Y si la noche se acaba, que no sea hoy",
        "Quédate aquí, quédate un poco más",
        "Tus ojos dicen lo que callas",
        "Esta canción es solo para ti",
    ];
    let mut out = vec![Line {
        at: 0.0,
        text: format!("♪ {title}"),
    }];
    let mut at = 8.0;
    let mut i = title.len();
    while at < secs as f32 - 6.0 {
        out.push(Line {
            at,
            text: LINES[i % LINES.len()].into(),
        });
        at += 7.5;
        i += 3;
    }
    out
}

fn main() -> iced::Result {
    // Used by platforms/linux/install.sh, so the .desktop text comes from the toolkit.
    if std::env::args().nth(1).as_deref() == Some("--print-desktop-entry") {
        print!("{}", platforms::DESKTOP_ENTRY.render());
        return Ok(());
    }
    // Shared desktop behavior: ~/.config/iced-adaptive-design/desktop.conf (+ system files).
    iad::config::install(iad::config::Config::load());
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(window_size().0, window_size().1),
            ..iad::windows::settings_for(platforms::APP_ID)
        })
        .settings(iced::Settings {
            default_text_size: 14.67.into(),
            antialiasing: true,
            ..Default::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let (backend, lib) = backend::sample::backend();
        App::with(backend, lib).0
    }

    #[test]
    fn back_returns_to_the_previous_page_not_a_parent() {
        let mut a = app();
        // Home -> an artist -> one of its albums -> back is the artist, then Home.
        let _ = a.update(Message::Open(Dest::Artist(1)));
        let _ = a.update(Message::Open(Dest::Album(2)));
        assert_eq!(a.dest, Dest::Album(2));
        let _ = a.update(Message::Back);
        assert_eq!(a.dest, Dest::Artist(1));
        let _ = a.update(Message::Back);
        assert_eq!(a.dest, Dest::Home);
        assert!(!a.history.can_back());
    }

    #[test]
    fn back_restores_a_search_and_top_level_navigation_starts_over() {
        let mut a = app();
        let _ = a.update(Message::SearchOpen(true));
        let _ = a.update(Message::SearchText("ave".into()));
        let _ = a.update(Message::Open(Dest::Playlist(0)));
        assert!(!a.searching);
        let _ = a.update(Message::Back);
        assert!(a.searching);
        assert_eq!(a.query, "ave");
        let _ = a.update(Message::Open(Dest::Artist(0)));
        let _ = a.update(Message::Go(Dest::Tracks));
        assert!(
            !a.history.can_back(),
            "a top-level destination clears the history"
        );
    }

    #[test]
    fn density_is_the_users_choice_or_the_desktops_for_every_list() {
        let mut a = app();
        let desktop = iad::config::current().density == iad::config::Density::Compact;
        assert_eq!(a.compact(), desktop, "follows the desktop by default");
        let _ = a.update(Message::SetDensity(DensityPref::Compact));
        assert!(a.compact());
        assert_eq!(a.row_h(), ROW_COMPACT);
        // the jam's list uses the same height
        let _ = a.update(Message::ToggleJam);
        assert_eq!(a.row_h(), ROW_COMPACT);
        let _ = a.update(Message::ToggleJam);
        assert!(!a.jam_open, "the jam button toggles the page");
        let _ = a.update(Message::SetDensity(DensityPref::Default));
        assert!(!a.compact());
        assert_eq!(a.row_h(), ROW);
    }

    #[test]
    fn the_playing_track_is_not_in_up_next_and_the_rest_is_always_editable() {
        let mut a = app();
        a.play((0..a.lib.tracks.len()).collect(), 3);
        let cur = a.player.current.unwrap();
        let playing = a.now().map(|t| t.id).unwrap();
        // Up next is what follows the playing track; the playing one is not in it.
        assert_eq!(
            a.player.up_next.items().len(),
            a.player.queue.len() - cur - 1
        );
        assert!(!a.player.up_next.items().contains(&playing));
        // removing the last upcoming row
        let n = a.player.queue.len();
        let last = a.player.up_next.items().len() - 1;
        let _ = a.update(Message::QueueRemove(last));
        assert_eq!(a.player.queue.len(), n - 1);
        assert_eq!(
            a.now().map(|t| t.id),
            Some(playing),
            "the playing track is untouched"
        );
        // removing the first upcoming row never touches the playing track either
        let _ = a.update(Message::QueueRemove(0));
        assert_eq!(a.now().map(|t| t.id), Some(playing));
        assert_eq!(a.player.queue[cur], playing);
    }

    #[test]
    fn a_playlist_is_renamed_and_described_through_the_engine() {
        let (mut backend, _) = backend::sample::backend();
        backend::sample::playlist(&mut backend, "Road trip", "For the drive");
        backend::sample::playlist(&mut backend, "Other", "");
        let lib = backend.load().unwrap();
        let mut a = App::with(backend, lib).0;
        let at = |a: &App, name: &str| a.lib.playlists.iter().position(|p| p.name == name);
        let i = at(&a, "Road trip").unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(i)));
        // One form for the name and the description, saved together.
        let _ = a.update(Message::EditDetails);
        let draft = a.edit.clone().unwrap();
        assert_eq!(draft.name.as_deref(), Some("Road trip"));
        assert_eq!(draft.description, "For the drive");
        let _ = a.update(Message::NameInput("Alpha drive".into()));
        let _ = a.update(Message::DescInput("Slow songs".into()));
        // Nothing is saved until Save: Cancel drops the draft.
        let _ = a.update(Message::CancelDetails);
        assert!(a.edit.is_none() && at(&a, "Road trip").is_some());
        let _ = a.update(Message::EditDetails);
        let _ = a.update(Message::NameInput("Alpha drive".into()));
        let _ = a.update(Message::DescInput("Slow songs".into()));
        let _ = a.update(Message::SaveDetails);
        assert!(a.edit.is_none());
        // Saved in the engine, and the page follows the playlist to where the list sorts it.
        let i = at(&a, "Alpha drive").expect("renamed");
        assert!(at(&a, "Road trip").is_none());
        assert_eq!(a.dest, Dest::Playlist(i));
        assert_eq!(a.lib.playlists[i].description, "Slow songs");
        // Reading the library again shows the engine has both.
        let _ = a.update(Message::Imported(Ok(String::new())));
        let i = at(&a, "Alpha drive").unwrap();
        assert_eq!(a.lib.playlists[i].description, "Slow songs");
        // Another playlist's name is refused, and nothing changes.
        let _ = a.update(Message::Open(Dest::Playlist(i)));
        let _ = a.update(Message::EditDetails);
        let _ = a.update(Message::NameInput("other".into()));
        let _ = a.update(Message::SaveDetails);
        assert!(a.notice.as_ref().unwrap().0.contains("Cannot rename"));
        assert!(at(&a, "Alpha drive").is_some());
        // Artists and albums edit the description only: there is no name to edit.
        let _ = a.update(Message::Open(Dest::Artist(0)));
        let _ = a.update(Message::EditDetails);
        assert!(a.edit.as_ref().is_some_and(|d| d.name.is_none()));
        let _ = a.view();
        let _ = a.update(Message::DescInput("A band.".into()));
        let _ = a.update(Message::SaveDetails);
        assert_eq!(
            a.descriptions.get(&a.desc_key().unwrap()).unwrap(),
            "A band."
        );
    }

    #[test]
    fn a_pages_options_open_as_a_sheet_and_edit_details_opens_the_dialog() {
        let (mut backend, _) = backend::sample::backend();
        backend::sample::playlist(&mut backend, "Road trip", "For the drive");
        let lib = backend.load().unwrap();
        let mut a = App::with(backend, lib).0;
        let _ = a.update(Message::Open(Dest::Playlist(0)));
        let _ = a.update(Message::OpenSheet(Some(Sheet::Options(false))));
        assert_eq!(a.sheet, Some(Sheet::Options(false)));
        let _ = a.view();
        // A choice in the sheet closes it, then does it.
        let _ = a.update(Message::SheetAction(Box::new(Message::EditDetails)));
        assert!(a.sheet.is_none());
        assert_eq!(
            a.edit.as_ref().and_then(|d| d.name.as_deref()),
            Some("Road trip")
        );
        let _ = a.view();
    }

    #[test]
    fn edit_details_is_a_dialog_when_wide_and_a_bottom_sheet_when_narrow() {
        let (mut backend, _) = backend::sample::backend();
        backend::sample::playlist(&mut backend, "Road trip", "For the drive");
        let lib = backend.load().unwrap();
        let mut a = App::with(backend, lib).0;
        let _ = a.update(Message::Open(Dest::Playlist(0)));
        let _ = a.update(Message::EditDetails);
        assert!(!a.is_narrow());
        let _ = a.view();
        assert!(a.dialog_open_for_test(), "a dialog when wide");
        let _ = a.update(Message::Resized(400.0, 800.0));
        assert!(a.is_narrow());
        let _ = a.view();
        assert!(
            !a.dialog_open_for_test(),
            "no dialog when narrow: the sheet shows it"
        );
        // Cancel closes it in either layout.
        let _ = a.update(Message::CancelDetails);
        assert!(a.edit.is_none());
    }

    #[test]
    fn home_and_explore_say_what_will_show_up_when_there_is_no_history() {
        // The mock library has tracks but no listening history: those sections get a note
        // instead of staying blank, and every page builds.
        let (backend, mut lib) = backend::sample::backend();
        for t in &mut lib.tracks {
            t.plays = 0;
            t.last_played_days = 0;
        }
        let mut a = App::with(backend, lib).0;
        assert!(a.lib.recently_played(5).is_empty() && a.lib.recent_artists(8).is_empty());
        for dest in [Dest::Home, Dest::Explore] {
            let _ = a.update(Message::Open(dest));
            let _ = a.view();
        }
    }

    #[test]
    fn an_empty_library_opens_and_every_page_builds() {
        let (backend, lib) = backend::sample::empty();
        let mut a = App::with(backend, lib).0;
        for dest in [
            Dest::Home,
            Dest::Tracks,
            Dest::Recent,
            Dest::Artists,
            Dest::Albums,
            Dest::Playlists,
            Dest::Explore,
        ] {
            let _ = a.update(Message::Open(dest));
            let _ = a.view();
        }
        let _ = a.update(Message::TogglePlay);
        let _ = a.update(Message::Next);
        assert!(a.player.queue.is_empty());
    }

    #[test]
    fn list_pages_show_an_empty_page_instead_of_a_blank_one() {
        // A library with tracks but no playlists and no listening history.
        let (backend, mut lib) = backend::sample::backend();
        for t in &mut lib.tracks {
            t.plays = 0;
        }
        let mut a = App::with(backend, lib).0;
        assert!(a.lib.playlists.is_empty() && a.lib.recently_played(1).is_empty());
        for dest in [
            Dest::Playlists,
            Dest::Recent,
            Dest::Explore,
            Dest::Artists,
            Dest::Albums,
        ] {
            let _ = a.update(Message::Open(dest));
            let _ = a.view();
        }
        // Without a history Recent tracks falls back to what was added last, so it is not blank.
        let _ = a.update(Message::Open(Dest::Recent));
        assert!(!a.rows.is_empty());
    }

    #[test]
    fn recent_tracks_follow_what_is_played_and_can_be_ordered_by_when_added() {
        // The sample library has a made-up history; start from a library nothing was played in.
        let (backend, mut lib) = backend::sample::backend();
        lib.apply_history(&[], 0);
        let mut a = App::with(backend, lib).0;
        let _ = a.update(Message::Open(Dest::Recent));
        // Nothing played: the tracks added last, newest first.
        let added: Vec<usize> = a.lib.recently_added(30).iter().map(|t| t.id).collect();
        assert_eq!(a.rows.iter().map(|t| t.id).collect::<Vec<_>>(), added);
        let _ = a.view();
        // Playing a track is a play in the engine: it comes first now, whatever its age.
        let oldest = *added.last().unwrap();
        a.play(vec![oldest], 0);
        assert_eq!(a.lib.tracks[oldest].plays, 1);
        assert_eq!(a.rows.first().map(|t| t.id), Some(oldest));
        // Ordered by when added, the recent tracks are the added ones again.
        let _ = a.update(Message::SetRecentOrder(RecentOrder::Added));
        assert_eq!(a.rows.iter().map(|t| t.id).collect::<Vec<_>>(), added);
        let _ = a.view();
        // Both layouts have the menu: the dropdown, and the sheet.
        let _ = a.update(Message::Resized(400.0, 800.0));
        let _ = a.update(Message::OpenSheet(Some(Sheet::Recent)));
        let _ = a.view();
        let _ = a.update(Message::SheetAction(Box::new(Message::SetRecentOrder(
            RecentOrder::Played,
        ))));
        assert!(a.sheet.is_none() && a.recent_order == RecentOrder::Played);
        assert_eq!(a.rows.first().map(|t| t.id), Some(oldest));
    }

    #[test]
    fn next_and_previous_are_available_only_when_they_have_somewhere_to_go() {
        let mut a = app();
        // Nothing playing: neither.
        assert!(!a.can_next() && !a.can_previous());
        a.play(vec![5, 6, 7], 0);
        // The first track, at its start: only Next.
        assert!(a.can_next() && !a.can_previous());
        let _ = a.update(Message::Next);
        assert!(a.can_next() && a.can_previous(), "in the middle both");
        let _ = a.update(Message::Next);
        // The last track: only Previous.
        assert!(!a.can_next() && a.can_previous());
        // Repeating the queue loops it, so both work at either end.
        let _ = a.update(Message::Repeat);
        assert!(a.player.repeat && a.can_next() && a.can_previous());
        let _ = a.update(Message::Repeat);
        // Far enough into the first track, Previous restarts it.
        a.play(vec![5, 6, 7], 0);
        assert!(!a.can_previous());
        let _ = a.update(Message::Seek(30.0));
        assert!(a.can_previous());
        // The controls build either way.
        let _ = a.view();
    }

    #[test]
    fn playing_a_track_moves_the_highlight_in_the_queue_and_never_clears_it() {
        let mut a = app();
        // Nothing queued: the page's list becomes the queue, so Next has somewhere to go.
        let _ = a.update(Message::PlayTrack(10));
        assert_eq!(a.player.queue.len(), a.lib.tracks.len());
        assert_eq!(a.now().map(|t| t.id), Some(10));
        // From here on the queue stays whatever is played.
        a.play(vec![5, 6, 7, 8], 1);
        // A queued track: the highlight moves to it, down the queue...
        let _ = a.update(Message::PlayTrack(8));
        assert_eq!(a.player.queue, vec![5, 6, 7, 8]);
        assert_eq!(a.player.current, Some(3));
        // ...a track that is not queued is added after the playing one, and the highlight goes there...
        let _ = a.update(Message::PlayOne(20));
        assert_eq!(a.player.queue, vec![5, 6, 7, 8, 20]);
        assert_eq!(a.player.current, Some(4));
        // ...and back up the queue to an earlier one.
        let _ = a.update(Message::PlayOne(5));
        assert_eq!(a.player.queue, vec![5, 6, 7, 8, 20]);
        assert_eq!(a.player.current, Some(0));
        assert!(a.player.playing);
        // The play button of a page still plays the whole list.
        let _ = a.update(Message::PlayAll(vec![30, 31]));
        assert_eq!(a.player.queue, vec![30, 31]);
    }

    #[test]
    fn with_shuffle_the_queue_shows_the_play_order_and_does_not_end_early() {
        let mut a = app();
        let ids: Vec<usize> = (5..=14).collect();
        a.play(ids.clone(), 0);
        assert_eq!(a.player.queue, ids, "list order without shuffle");
        let _ = a.update(Message::Shuffle);
        assert!(a.player.shuffle);
        // The queue now lists the play order: the same tracks, the playing one first, the rest
        // in a new order (the list itself is not reordered).
        let mut sorted = a.player.queue.clone();
        sorted.sort();
        assert_eq!(sorted, ids);
        assert_eq!(a.player.current, Some(0));
        assert_eq!(a.player.queue[0], 5);
        assert_ne!(a.player.queue, ids, "shuffled");
        // Playing a track that is further down the list sets a new order from there; the
        // tracks before it in the list are not lost.
        let _ = a.update(Message::PlayOne(14));
        assert_eq!(a.now().map(|t| t.id), Some(14));
        assert_eq!(a.player.current, Some(0));
        // Next walks the whole play order, whatever the list order was, and then ends.
        let mut seen = vec![14];
        while a.can_next() {
            let _ = a.update(Message::Next);
            seen.push(a.now().unwrap().id);
        }
        seen.sort();
        assert_eq!(seen, ids, "every track had its turn before the end");
        assert!(!a.can_next());
        // Dragging in "Up next" moves entries of the play order.
        let _ = a.update(Message::PlayOne(14));
        let upcoming: Vec<usize> = a.player.up_next.items().to_vec();
        let _ = a.update(Message::Queue(ReorderEvent::Reorder { from: 0, to: 2 }));
        let after: Vec<usize> = a.player.up_next.items().to_vec();
        assert_eq!((after[1], after[2]), (upcoming[2], upcoming[0]));
        assert_eq!(a.now().map(|t| t.id), Some(14));
        // Turning shuffle off lists the queue in list order again.
        let _ = a.update(Message::Shuffle);
        let mut list = a.player.queue.clone();
        assert_eq!(list.len(), ids.len());
        list.sort();
        assert_eq!(list, ids);
        assert_eq!(a.player.queue, ids);
    }

    #[test]
    fn the_volume_slider_sets_it_and_the_button_mutes_and_restores_it() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        assert_eq!(a.player.volume, 1.0);
        let _ = a.update(Message::SetVolume(0.4));
        assert_eq!(a.player.volume, 0.4);
        let _ = a.update(Message::ToggleMute);
        assert_eq!(a.player.volume, 0.0);
        let _ = a.update(Message::ToggleMute);
        assert_eq!(a.player.volume, 0.4, "back to what it was");
        // Muted with the slider (nothing remembered): the button brings sound back.
        let _ = a.update(Message::SetVolume(0.0));
        let _ = a.update(Message::ToggleMute);
        assert!(a.player.volume > 0.0);
    }

    #[test]
    fn the_queue_clears_what_follows_and_history_is_the_played_part() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        a.play(vec![10, 11, 12, 13, 14], 2);
        assert!(!a.history_open, "collapsed by default");
        let _ = a.update(Message::ClearQueue);
        // The playing track and what was played stay; only up next goes.
        assert_eq!(a.player.queue, vec![10, 11, 12]);
        assert_eq!(a.player.current, Some(2));
        assert!(a.player.up_next.items().is_empty());
        // A history entry plays again, and the queue keeps its tracks.
        let _ = a.update(Message::QueueJump(0));
        assert_eq!(a.player.current, Some(0));
        assert_eq!(a.player.queue, vec![10, 11, 12]);
        let _ = a.update(Message::ToggleQueueHistory);
        assert!(a.history_open);
        assert!(a.up_next_open, "up next is open until folded");
        let _ = a.update(Message::ToggleUpNext);
        assert!(!a.up_next_open);
    }

    #[test]
    fn full_screen_mode_shows_now_playing_and_leaving_puts_things_back() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        a.play(vec![10, 11], 0);
        let _ = a.update(Message::Fullscreen(true));
        assert!(a.fullscreen && a.now_playing, "fullscreen over Now Playing");
        let _ = a.update(Message::Fullscreen(false));
        assert!(
            !a.fullscreen && !a.now_playing,
            "Now Playing was not open before"
        );
        // Started from Now Playing: it stays open when full screen ends.
        let _ = a.update(Message::NowPlaying(true));
        let _ = a.update(Message::ToggleFullscreen);
        assert!(a.fullscreen);
        let _ = a.update(Message::EscapeKey);
        assert!(!a.fullscreen && a.now_playing);
        // Escape does nothing when not fullscreen; closing Now Playing ends full screen.
        let _ = a.update(Message::EscapeKey);
        assert!(a.now_playing);
        let _ = a.update(Message::Fullscreen(true));
        let _ = a.update(Message::NowPlaying(false));
        assert!(!a.fullscreen && !a.now_playing);
        let _ = a.view();
    }

    #[test]
    fn favourites_is_a_default_playlist_that_holds_the_favourited_tracks() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        assert!(!a.is_favourite(10));
        let _ = a.update(Message::ToggleFavoriteTrack(10));
        assert!(a.is_favourite(10), "the heart fills");
        let fav = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .expect("the playlist exists once something is favourited");
        assert_eq!(a.lib.playlists[fav].tracks, vec![10]);
        let _ = a.update(Message::ToggleFavoriteTrack(12));
        assert_eq!(a.lib.playlists[fav].tracks, vec![10, 12]);
        // It is kept by the engine: reading the library again finds it.
        assert!(a.reload_library());
        assert!(a.is_favourite(10) && a.is_favourite(12) && !a.is_favourite(11));
        let _ = a.update(Message::ToggleFavoriteTrack(10));
        assert!(!a.is_favourite(10) && a.is_favourite(12));
        // The default playlist keeps its name: Edit details has no name field for it.
        let fav = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(fav)));
        let _ = a.update(Message::EditDetails);
        assert!(a.edit.as_ref().is_some_and(|d| d.name.is_none()));
    }

    #[test]
    fn a_session_left_playing_starts_paused_with_its_queue_and_place_kept() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        a.play(vec![10, 11, 12], 1);
        assert!(a.player.playing, "it was playing when the window closed");
        // The next start.
        a.backend.start_paused();
        let s = a.backend.snapshot();
        assert!(!s.playing, "nothing sounds until asked");
        assert_eq!(s.queue, vec![10, 11, 12]);
        assert_eq!(s.current, Some(1));
        // Already paused (or never started): nothing changes.
        a.backend.start_paused();
        assert!(!a.backend.snapshot().playing);
    }

    #[test]
    fn the_jam_page_lists_only_active_filters_and_edits_them_in_a_dialog() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        let _ = a.update(Message::ToggleJam);
        assert!(
            a.jam_open && !a.jam_filters_open,
            "the filters start closed"
        );
        assert!(!a.dialog_open_for_test());
        let _ = a.update(Message::JamFilters(true));
        assert!(a.dialog_open_for_test(), "wide: a dialog");
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let _ = a.update(Message::Bpm((90.0, 120.0)));
        assert_eq!(a.filter.count(), 2);
        let _ = a.update(Message::JamFilters(false));
        assert!(!a.dialog_open_for_test());
        // An active chip clears on its own: a tag by toggling it, the tempo by resetting it.
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        assert_eq!(a.filter.count(), 1);
        let _ = a.update(Message::Bpm(data::BPM));
        assert!(a.filter.is_empty());
    }

    #[test]
    fn minimal_mode_shrinks_the_window_and_remembers_its_size_to_come_back() {
        let (backend, lib) = backend::sample::backend();
        let mut a = App::with(backend, lib).0;
        a.play(vec![10, 11], 0);
        let _ = a.update(Message::Resized(900.0, 640.0));
        let _ = a.update(Message::NowPlaying(true));
        let _ = a.update(Message::Minimal(true));
        assert!(a.minimal && !a.now_playing, "overlays close");
        assert_eq!(a.restore_size, Some((900.0, 640.0)));
        // The strip builds in both views, and the cover switches between them.
        let _ = a.view();
        assert!(!a.minimal_details, "the spectrum first");
        let _ = a.update(Message::MinimalDetails);
        assert!(a.minimal_details);
        let _ = a.view();
        let _ = a.update(Message::MinimalDetails);
        assert!(!a.minimal_details);
        let _ = a.update(Message::Resized(560.0, 72.0));
        let _ = a.update(Message::Minimal(false));
        assert!(!a.minimal);
        assert_eq!(a.restore_size, None, "the size is used once");
        // Asking for what already is does nothing.
        let _ = a.update(Message::Minimal(false));
        assert!(!a.minimal);
    }

    #[test]
    fn a_playlist_or_album_has_a_shuffle_toggle_that_decides_how_play_starts_it() {
        let (mut backend, _) = backend::sample::backend();
        backend::sample::playlist(&mut backend, "Road trip", "");
        let lib = backend.load().unwrap();
        let mut a = App::with(backend, lib).0;
        let ids: Vec<usize> = (0..10).collect();
        // Something else is queued, and the player itself is shuffling.
        a.play(vec![40, 41, 42], 0);
        let _ = a.update(Message::Shuffle);
        let _ = a.update(Message::Open(Dest::Playlist(0)));
        assert_eq!(a.page_shuffle(), Some(false), "off until it is switched on");
        // Off: Play starts the playlist in order, with shuffle off, replacing the queue.
        let _ = a.update(Message::PlayAll(ids.clone()));
        assert!(!a.player.shuffle);
        assert_eq!(a.player.queue, ids);
        assert_eq!(a.player.current, Some(0));
        // On: it starts shuffled (from a random track), replacing the queue.
        let _ = a.update(Message::SetShufflePref(Some(true)));
        assert_eq!(a.page_shuffle(), Some(true));
        a.play(vec![40, 41, 42], 0);
        let _ = a.update(Message::PlayAll(ids.clone()));
        assert!(a.player.shuffle);
        let mut queued = a.player.queue.clone();
        queued.sort();
        assert_eq!(queued, ids, "just the playlist is queued");
        // The toggle is kept in the engine: reading the library again keeps it.
        let _ = a.update(Message::Imported(Ok(String::new())));
        let i = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == "Road trip")
            .unwrap();
        assert_eq!(a.lib.playlists[i].shuffle, Some(true));
        let _ = a.update(Message::Open(Dest::Playlist(i)));
        let _ = a.update(Message::SetShufflePref(None));
        assert_eq!(a.page_shuffle(), Some(false));
        // An album has the same toggle, found again through its tracks.
        let _ = a.update(Message::Open(Dest::Album(0)));
        assert_eq!(a.page_shuffle(), Some(false));
        let _ = a.update(Message::SetShufflePref(Some(true)));
        let _ = a.update(Message::Imported(Ok(String::new())));
        let _ = a.update(Message::Open(Dest::Album(0)));
        assert_eq!(a.page_shuffle(), Some(true));
        let album: Vec<usize> = a.rows.iter().map(|t| t.id).collect();
        a.play(vec![40, 41, 42], 0);
        let _ = a.update(Message::PlayAll(album.clone()));
        assert!(a.player.shuffle);
        assert_eq!(a.player.queue.len(), album.len());
        // The artist page has none, and the pages build in both layouts.
        let _ = a.update(Message::Open(Dest::Artist(0)));
        assert_eq!(a.page_shuffle(), None);
        for dest in [Dest::Playlist(i), Dest::Album(0), Dest::Artist(0)] {
            let _ = a.update(Message::Open(dest));
            let _ = a.view();
            let _ = a.update(Message::Resized(400.0, 800.0));
            let _ = a.view();
            let _ = a.update(Message::Resized(1100.0, 800.0));
        }
    }

    #[test]
    fn tracks_with_no_album_tag_show_no_album_and_belong_to_none() {
        let (mut backend, _) = backend::sample::backend();
        backend::sample::untagged(&mut backend, &["plain a", "plain b"]);
        let lib = backend.load().unwrap();
        let mut a = App::with(backend, lib).0;
        // Nothing is made up for them: an empty album, no album page, and a line without one.
        let plain: Vec<&Track> = a
            .lib
            .tracks
            .iter()
            .filter(|t| t.title.starts_with("plain"))
            .collect();
        assert_eq!(plain.len(), 2);
        assert!(plain.iter().all(|t| t.album.is_empty()));
        assert_eq!(plain[0].by_line(), "x", "just the artist, no separator");
        assert!(a.lib.albums().iter().all(|(album, _, _)| !album.is_empty()));
        assert!(
            !a.lib
                .albums()
                .iter()
                .any(|(album, _, _)| album.contains("Unknown"))
        );
        // A library of only untagged tracks has no albums, and its pages say so.
        let (mut backend, _) = backend::sample::empty();
        backend::sample::untagged(&mut backend, &["solo"]);
        let lib = backend.load().unwrap();
        let mut only = App::with(backend, lib).0;
        assert!(only.lib.albums().is_empty());
        for dest in [Dest::Albums, Dest::Home, Dest::Explore] {
            let _ = only.update(Message::Open(dest));
            let _ = only.view();
        }
        // Every album page is a real album, so each has the shuffle toggle.
        for i in 0..a.lib.albums().len() {
            let _ = a.update(Message::Open(Dest::Album(i)));
            assert!(a.page_shuffle().is_some());
        }
        // The pages that list them build, in both layouts, and the info and track dialogs too.
        let id = a
            .lib
            .tracks
            .iter()
            .position(|t| t.title == "plain a")
            .unwrap();
        for dest in [Dest::Tracks, Dest::Albums, Dest::Home] {
            let _ = a.update(Message::Open(dest));
            let _ = a.view();
        }
        let _ = a.update(Message::InfoTrack(id));
        let _ = a.view();
        let _ = a.update(Message::CloseInfo);
        a.play(vec![id], 0);
        let _ = a.update(Message::Resized(400.0, 800.0));
        let _ = a.view();
    }

    #[test]
    fn the_player_mirrors_the_engine_session() {
        let mut a = app();
        assert!(a.player.queue.is_empty() && a.player.current.is_none() && !a.player.playing);
        // Playing a list queues it in the engine and starts at the chosen track.
        a.play(vec![5, 6, 7, 8], 1);
        assert_eq!(a.player.queue, vec![5, 6, 7, 8]);
        assert_eq!((a.player.current, a.player.playing), (Some(1), true));
        assert_eq!(a.now().map(|t| t.id), Some(6));
        // Transport goes through the engine and comes back.
        let _ = a.update(Message::TogglePlay);
        assert!(!a.player.playing);
        let _ = a.update(Message::TogglePlay);
        assert!(a.player.playing);
        let _ = a.update(Message::Next);
        assert_eq!(a.now().map(|t| t.id), Some(7));
        let _ = a.update(Message::Seek(30.0));
        assert!(a.player.pos >= 30.0);
        let _ = a.update(Message::Shuffle);
        assert!(a.player.shuffle);
        let _ = a.update(Message::Repeat);
        assert!(a.player.repeat);
        // Queueing next puts a track right after the playing one.
        let playing = a.now().map(|t| t.id).unwrap();
        let _ = a.update(Message::PlayNext(20));
        let at = a.player.queue.iter().position(|i| *i == playing).unwrap();
        assert_eq!(a.player.queue[at + 1], 20);
        // A drop in "Up next" reorders the engine queue and keeps the playing track.
        let before: Vec<usize> = a.player.up_next.items().to_vec();
        let _ = a.update(Message::Queue(ReorderEvent::Reorder { from: 0, to: 1 }));
        let after: Vec<usize> = a.player.up_next.items().to_vec();
        assert_eq!((after[0], after[1]), (before[1], before[0]));
        assert_eq!(a.now().map(|t| t.id), Some(playing));
    }

    #[test]
    fn on_desktop_unselecting_a_header_button_returns_home() {
        let mut a = app();
        assert!(!a.is_narrow());
        let _ = a.update(Message::Open(Dest::Artist(1)));
        let _ = a.update(Message::ToggleExplore);
        assert_eq!(a.dest, Dest::Explore);
        let _ = a.update(Message::ToggleExplore);
        assert_eq!(a.dest, Dest::Home);
        let _ = a.update(Message::Go(Dest::Tracks));
        let _ = a.update(Message::ToggleJam);
        assert!(a.jam_open);
        let _ = a.update(Message::ToggleJam);
        assert!(!a.jam_open);
        assert_eq!(
            a.dest,
            Dest::Home,
            "closing the jam goes Home, not back to Tracks"
        );
        let _ = a.update(Message::SearchOpen(true));
        let _ = a.update(Message::SearchOpen(false));
        assert!(!a.searching);
        assert_eq!(a.dest, Dest::Home);
    }

    #[test]
    fn a_jam_can_be_saved_as_a_dynamic_playlist() {
        let mut a = app();
        let before = a.lib.playlists.len();
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let _ = a.update(Message::SaveJamAsPlaylist);
        assert_eq!(a.lib.playlists.len(), before + 1);
        let p = a.lib.playlists.last().unwrap();
        assert_eq!(
            p.filter.as_ref(),
            Some(&a.filter),
            "stores the filters, not the tracks"
        );
        assert!(p.tracks.is_empty());
        assert!(p.name.contains("romantic"), "{}", p.name);
        // saving the same filters again makes a second playlist with its own name
        let first = p.name.clone();
        a.jam_saved = None;
        let _ = a.update(Message::SaveJamAsPlaylist);
        assert_ne!(a.lib.playlists.last().unwrap().name, first);
    }

    #[test]
    fn the_menu_switches_density_and_an_import_reloads_the_library() {
        let mut a = app();
        let _ = a.update(Message::SetDensity(DensityPref::Compact));
        assert!(a.compact());
        let _ = a.update(Message::SetDensity(DensityPref::Default));
        assert!(!a.compact());
        // A finished import shows its result and reads the library again; the queue keeps
        // its tracks by slug.
        a.play(vec![5, 6, 7], 1);
        let playing = a.now().map(|t| t.slug.clone());
        let _ = a.update(Message::Imported(Ok("Imported 2 tracks".into())));
        assert_eq!(
            a.notice.as_ref().map(|n| n.0.as_str()),
            Some("Imported 2 tracks")
        );
        assert_eq!(a.now().map(|t| t.slug.clone()), playing);
        let _ = a.update(Message::Imported(Err("boom".into())));
        assert!(a.notice.as_ref().unwrap().0.contains("boom"));
    }

    #[test]
    fn the_queue_and_full_screen_now_playing_never_show_together() {
        let mut a = app();
        let _ = a.update(Message::NowPlaying(true));
        assert!(a.now_playing);
        let _ = a.update(Message::ToggleQueue);
        assert!(
            a.queue_open && !a.now_playing,
            "opening the queue collapses Now Playing"
        );
        let _ = a.update(Message::NowPlaying(true));
        assert!(
            a.now_playing && !a.queue_open,
            "expanding Now Playing leaves the queue"
        );
        let _ = a.update(Message::ToggleQueue);
        let _ = a.update(Message::ToggleQueue);
        assert!(
            !a.queue_open && !a.now_playing,
            "closing the queue does not reopen Now Playing"
        );
    }

    #[test]
    fn the_queue_slide_opens_and_closes_toward_its_target() {
        let mut sl = Slide::default();
        assert!(!sl.mounted());
        let t0 = Instant::now();
        let mut last = 0.0;
        for i in 0..80u32 {
            sl.drive(true, t0 + Duration::from_millis(16) * i);
            assert!(sl.shown() >= last - 1e-6, "opening never goes backwards");
            last = sl.shown();
        }
        assert_eq!(sl.shown(), 1.0);
        // closing: still mounted partway, gone at the end
        let t1 = t0 + Duration::from_millis(16) * 80;
        sl.drive(false, t1);
        sl.drive(false, t1 + Duration::from_millis(40));
        if iad::animation::enabled() {
            assert!(sl.mounted() && sl.shown() < 1.0, "mounted while it closes");
        }
        for i in 0..80u32 {
            sl.drive(false, t1 + Duration::from_millis(16) * (i + 3));
        }
        assert_eq!(sl.shown(), 0.0);
        assert!(!sl.mounted());
    }

    #[test]
    fn the_app_animates_only_while_the_queue_is_between_states() {
        let mut a = app();
        assert!(!a.animating());
        let _ = a.update(Message::ToggleQueue);
        assert!(
            a.animating(),
            "opening: the target moved, the slide has not caught up"
        );
        let t0 = Instant::now();
        for i in 0..80u32 {
            let _ = a.update(Message::Frame(t0 + Duration::from_millis(16) * i));
        }
        assert!(!a.animating());
        assert_eq!(a.queue_slide.shown(), 1.0);
        let _ = a.update(Message::ToggleQueue);
        assert!(a.animating());
    }
}
