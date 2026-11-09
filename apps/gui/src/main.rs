//! Weave's GUI frontend: iced with iced-adaptive-design (`iad`). See `docs/ui`.
//!
//! The UI runs on the engine: `data.rs` is a snapshot of the library database and `backend.rs`
//! drives the queue and transport. The application state (destination, search, jam, queue) lives
//! here; the layouts in `ui.rs` only render it, so a
//! window that crosses the breakpoint loses nothing.

mod appearance;
mod backend;
mod components;
mod data;
mod icons;
mod import_picker;
mod platforms;
mod ui;
mod ui_dialogs;
mod ui_library;
mod ui_lists;
mod ui_player;
mod ui_search;

use std::time::{Duration, Instant};

use data::{Filter, Library, RecentOrder, Track};
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

/// Row heights of every track list: the normal one carries a picture and two lines.
pub const ROW: f32 = 56.0;
pub const ROW_COMPACT: f32 = 32.0;

const WINDOW: (f32, f32) = (1120.0, 720.0);

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
    Rediscover,
    AboutMe,
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
        Self { dest }
    }
}

/// Where the user was: what Back returns to. Detail pages are opened from anywhere (Home, a
/// search, another detail page), so Back is the previous place, not a fixed parent.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub dest: Dest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterTarget {
    Jam,
    Draft,
    Page,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The desktop switched between light and dark; the theme is read again on redraw.
    SystemTheme(iad::utils::ThemeSettings),
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
    /// The search icon of the Jam page: shows or hides its artist, album and anything entries.
    JamSearchOpen(bool),
    SearchOpen(bool),
    SearchText(String),
    /// A chip of the Artists or Albums section of the Tag filters (any of the chosen ones).
    JamArtistToggle(String),
    JamAlbumToggle(String),
    /// What is typed to find an artist or an album in those sections.
    ArtistQuery(String),
    AlbumQuery(String),
    /// The same chips in the playlist-filter editor.
    DraftArtistToggle(String),
    DraftAlbumToggle(String),
    /// Save as playlist: opens the dialog (or sheet) that asks for the name.
    SaveNameInput(String),
    ConfirmSaveJam,
    CancelSaveJam,
    /// The generic search over title, artist and album (not saved with a playlist).
    JamQuery(String),
    JamRecent(bool),
    DraftRecent(bool),
    /// The jam is a page: this shows it, or goes back to the page you were on.
    ToggleJam,
    ToggleExplore,
    AnalyticsLoaded(
        u64,
        Box<Result<weave_player_lib::core::ListeningAnalytics, String>>,
    ),
    CollectionQuery(String),
    CollectionOrder(weave_player_lib::core::CollectionOrder),
    ExploreSection(usize),
    ArtistSection(usize),
    PageFilters(bool),
    PageTag(String),
    PageArtist(String),
    PageAlbum(String),
    PageBpm((f32, f32)),
    PageRecent(bool),
    Exclusions(String),
    DraftExclusions(String),
    PageExclusions(String),
    PageRating(u8),
    RediscoverDays(String),
    RediscoverSort(weave_player_lib::core::SortField),
    AnalyticsPeriod(Option<weave_player_lib::core::AnalyticsPeriod>),
    AnalyticsDateText(String),
    AnalyticsDatePick(iad::primitives::range::DateRange),
    AnalyticsDateView(iad::primitives::date::Date),

    /// Open the desktop file or folder chooser directly.
    Import(import_picker::Mode),
    ImportChosen(u64, Vec<std::path::PathBuf>),
    DismissImportReport,
    /// A confirmed mixed selection. Empty input does not start an import.
    Picked(Vec<std::path::PathBuf>),
    /// The import finished: what to tell the user.
    Imported(Result<weave_player_lib::core::ImportReport, String>),
    /// The bottom sheet of a menu (narrow): a track's actions or a page's options.
    OpenSheet(Option<Sheet>),
    /// How Recent tracks is ordered: by play or by when added.
    SetRecentOrder(RecentOrder),
    /// How the open playlist or album is played: shuffled, in order, or as the player says.
    SetShufflePref(Option<bool>),
    /// A choice made in the sheet: it closes, then does it.
    SheetAction(Box<Message>),
    SetDensity(DensityPref),
    ThemeMode(Option<iad::Scheme>),
    Themes(bool),
    ImportTheme,
    ThemeChosen(Option<std::path::PathBuf>),
    SelectTheme(Option<String>),
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
    /// Escape: leaves fullscreen or closes inline search.
    EscapeKey,
    ToggleLyrics,
    // The filter page, shared by search and jam.
    ToggleTag(String),
    CommitTagGroup(FilterTarget),
    SelectTagGroup(FilterTarget, Option<(usize, usize)>),
    FilterTagToggle(FilterTarget, String),
    RemoveGroupedTag(FilterTarget, usize, usize, String),
    /// Toggles a tag in the editable group at (block, index), making it the active one.
    ToggleInTagGroup(FilterTarget, (usize, usize), String),
    /// Does nothing (an event the controls send that has no effect here).
    Noop,
    TagQuery(String),
    TagBrowseToggle,
    TagCategories,
    TagCategory(Option<String>),
    ChipExpand(ChipPicker, bool),
    Bpm((f32, f32)),
    /// The clear icon of the Tag filters: drops the tags and the tempo, and leaves the search.
    ClearTagFilters,
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
    /// Delete playlist, in a playlist page's options: asks first.
    AskDeletePlaylist,
    ConfirmDeletePlaylist,
    CancelDeletePlaylist,
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
    ConfirmManualFilter,
    CancelManualFilterReview,
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
    /// A new reading of the spectrum while the Now Playing ring is visible.
    SpectrumTick,
    /// The volume button: mute, or back to the volume before.
    ToggleMute,
    Queue(ReorderEvent),
    PlaylistOrder(ReorderEvent),
    NewManualPlaylist,
    PlaylistTracksOpen(bool),
    PlaylistTracksQuery(String),
    PlaylistTrackToggle(usize),
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

/// Presentation state for each independent wrapping chip collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipPicker {
    Tags,
    Artists,
    Albums,
    TrackTags,
}

/// Which portion of the canonical tag vocabulary the picker presents. Selection is separate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TagBrowse {
    #[default]
    All,
    Categories,
    /// A tag key; None means unkeyed (Custom) tags.
    Category(Option<String>),
}

#[derive(Debug, Clone)]
pub struct ManualFilterReview {
    name: String,
    filter: Filter,
    expected: Vec<String>,
    removed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PlaylistOrderEdit {
    pub name: String,
    pub order: ReorderState<String>,
}

/// What the bottom sheet (narrow) shows: a menu that is a dropdown on desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    /// The top-bar settings and import menu.
    Main,
    /// A track's actions: the track, and whether "View album" shows.
    Track(usize, bool),
    /// A detail page's options: whether the page is a dynamic playlist.
    Options(bool),
    /// The order of Recent tracks.
    Recent,
    Sort,
    Sleep,
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
    /// The Jam page's search row (artist, album, anything) is open.
    pub jam_search_open: bool,
    pub search_open: bool,
    /// What is typed to find an artist / an album among the chips of the Tag filters.
    pub artist_query: String,
    pub album_query: String,
    /// The name being asked for by the Save as playlist dialog; `None` when it is closed.
    pub save_name: Option<String>,
    pub save_manual: bool,
    pub creation_filter: Filter,
    pub manual_filter_review: Option<ManualFilterReview>,
    pub playlist_tracks_open: bool,
    pub playlist_tracks_query: String,
    pub filter: Filter,
    pub collection_query: String,
    pub playlist_edit: Option<PlaylistOrderEdit>,
    pub detail_order: Option<weave_player_lib::core::CollectionOrder>,
    pub detail_descending: bool,
    /// Sort-icon choices are session-only playback rules, separate from saved filters.
    pub jam_order: Option<weave_player_lib::core::CollectionOrder>,
    pub jam_descending: bool,
    pub collection_rule_order: [bool; 4],
    pub collection_descending: [bool; 4],
    pub collection_orders: [Option<weave_player_lib::core::CollectionOrder>; 4],
    pub explore_open: [bool; 5],
    pub artist_open: std::collections::HashMap<String, [bool; 3]>,
    pub page_filters_open: bool,
    pub rediscover_filter: Filter,
    pub rediscover_days: String,
    pub rediscover_sort: weave_player_lib::core::SortField,
    pub analytics_filter: Filter,
    pub analytics_period: Option<weave_player_lib::core::AnalyticsPeriod>,
    pub analytics_date_text: String,
    pub analytics_range: iad::primitives::range::DateRange,
    pub analytics_date_view: iad::primitives::date::Date,
    pub analytics_result: Option<Result<weave_player_lib::core::ListeningAnalytics, String>>,
    pub analytics_request: u64,
    pub analytics_last_query: Option<weave_player_lib::core::AnalyticsQuery>,
    pub analytics_loading: bool,
    pub last_analytics: Instant,
    pub query_error: Option<String>,
    pub explicit_sort: bool,
    /// Narrows the tag chips of the filter page as you type.
    pub tag_query: String,
    pub active_tag_group: Option<(FilterTarget, usize, usize)>,
    pub tag_browse: TagBrowse,
    pub import_report: Option<weave_player_lib::core::ImportReport>,
    pub import_chooser_open: bool,
    pub import_request: u64,
    pub import_in_progress: bool,
    pub chip_expanded: [bool; 4],
    /// Default tag-chip lines before the expand chevron.
    pub chip_line_limit: usize,
    /// Default artist/album-chip lines before the expand chevron.
    pub name_chip_line_limit: usize,
    pub jam_open: bool,
    pub now_playing: bool,
    /// The playlist the Delete playlist question is about; `None` when it is not asked.
    pub delete_playlist: Option<String>,
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
    /// The spectrum of what plays, as last read (drawn by the Now Playing visualizer).
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
    /// Application override; None follows the desktop, Some forces light or dark.
    pub theme_mode: Option<iad::Scheme>,
    /// Latest desktop event; kept in app state so mode changes invalidate the view.
    pub desktop_scheme: iad::Scheme,
    pub themes: appearance::ThemeLibrary,
    pub theme_import_pending: bool,
    pub themes_open: bool,
    pub theme_config: appearance::ThemeConfig,
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
        let themes = appearance::ThemeLibrary::load(appearance::config_directory());
        let theme_config = themes
            .current()
            .map(|theme| theme.config.clone())
            .unwrap_or_else(|| appearance::config(""));
        let mut app = Self {
            themes,
            theme_config,
            theme_import_pending: false,
            themes_open: false,
            backend,
            adaptive: Adaptive::new(
                Layouts::new()
                    .layout(..WidthClass::Md, Layout::Narrow)
                    .layout(WidthClass::Md.., Layout::Wide),
            )
            .overlap(true),
            dest: Dest::Home,
            jam_search_open: false,
            search_open: false,
            artist_query: String::new(),
            album_query: String::new(),
            save_name: None,
            save_manual: false,
            creation_filter: Filter::default(),
            manual_filter_review: None,
            playlist_tracks_open: false,
            playlist_tracks_query: String::new(),
            filter: Filter::default(),
            collection_query: String::new(),
            playlist_edit: None,
            detail_order: None,
            detail_descending: true,
            jam_order: None,
            jam_descending: true,
            collection_rule_order: [false; 4],
            collection_descending: [true; 4],
            collection_orders: [Some(weave_player_lib::core::CollectionOrder::Recent); 4],
            explore_open: [true; 5],
            artist_open: std::collections::HashMap::new(),
            page_filters_open: false,
            rediscover_filter: Filter::default(),
            rediscover_days: "180".into(),
            rediscover_sort: weave_player_lib::core::SortField::Preference,
            analytics_filter: Filter::default(),
            analytics_period: Some(weave_player_lib::core::AnalyticsPeriod::Week),
            analytics_date_text: String::new(),
            analytics_range: Default::default(),
            analytics_date_view: iad::primitives::date::Date::today(),
            query_error: None,
            explicit_sort: false,
            analytics_result: None,
            analytics_request: 0,
            analytics_last_query: None,
            analytics_loading: false,
            last_analytics: Instant::now(),
            tag_query: String::new(),
            active_tag_group: None,
            tag_browse: TagBrowse::Categories,
            import_report: None,
            import_chooser_open: false,
            import_request: 0,
            import_in_progress: false,
            chip_expanded: [false; 4],
            chip_line_limit: 3,
            name_chip_line_limit: 2,
            jam_open: false,
            now_playing: false,
            delete_playlist: None,
            fullscreen: false,
            np_before_fullscreen: false,
            history_open: false,
            up_next_open: true,
            muted_from: None,
            queue_open: false,
            queue_slide: Slide::default(),
            win_h: window_size().1,
            win_w: window_size().0,
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
            theme_mode: None,
            desktop_scheme: iad::utils::theme_settings().scheme,
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
        if let Ok(start) = std::env::var("WEAVE_GUI_START") {
            for m in match start.as_str() {
                "themes" => vec![Message::Themes(true)],
                "jam" => vec![Message::ToggleJam],
                "jam-filters" => vec![Message::ToggleJam, Message::JamFilters(true)],
                "tag-categories" => vec![
                    Message::ToggleJam,
                    Message::JamFilters(true),
                    Message::TagCategories,
                ],
                "tag-genre" => vec![
                    Message::ToggleJam,
                    Message::JamFilters(true),
                    Message::TagCategory(Some("genre".into())),
                ],
                "tag-custom" => vec![
                    Message::ToggleJam,
                    Message::JamFilters(true),
                    Message::TagCategory(None),
                ],
                "jam-filters-expanded" => vec![
                    Message::ToggleJam,
                    Message::JamFilters(true),
                    Message::ChipExpand(ChipPicker::Tags, true),
                    Message::ChipExpand(ChipPicker::Artists, true),
                    Message::ChipExpand(ChipPicker::Albums, true),
                ],
                "jam-search" => vec![
                    Message::ToggleJam,
                    Message::JamSearchOpen(true),
                    Message::JamQuery("a".into()),
                ],
                "jam-picks" => vec![
                    Message::ToggleJam,
                    Message::JamArtistToggle("Aventura".into()),
                    Message::JamAlbumToggle("Don't Look Back".into()),
                    Message::ToggleTag("mood:romantic".into()),
                ],
                "jam-artists" => vec![
                    Message::ToggleJam,
                    Message::JamArtistToggle("Aventura".into()),
                    Message::JamAlbumToggle("Don't Look Back".into()),
                    Message::JamFilters(true),
                ],
                "jam-save" => vec![
                    Message::ToggleJam,
                    Message::ToggleTag("mood:romantic".into()),
                    Message::JamArtistToggle("Aventura".into()),
                    Message::JamQuery("e".into()),
                    Message::SaveJamAsPlaylist,
                ],
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
                "playlist" => vec![Message::Go(Dest::Playlist(0))],
                "playlist-in" => vec![Message::Open(Dest::Playlist(0))],
                "edit-track" => vec![Message::Go(Dest::Tracks), Message::EditTrack(3)],
                "describe" => vec![Message::Go(Dest::Artist(1)), Message::EditDetails],
                "delete-playlist" => {
                    vec![Message::Go(Dest::Playlist(0)), Message::AskDeletePlaylist]
                }
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
                "volume" => vec![Message::SetVolume(0.42)],
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
            let window = iced::window::oldest();
            tasks.push(
                window
                    .and_then(move |id| dev.capture(id).unwrap_or_else(Task::none))
                    .map(Message::Captured),
            );
        }
        tasks.push(iad::utils::detect_theme().map(Message::SystemTheme));
        (app, Task::batch(tasks))
    }

    /// Layouts here answer their transition commands at once.
    fn dispatch(&mut self, cmds: Vec<iad::transition::Command<Layout>>) -> Task<Message> {
        Task::batch(
            cmds.into_iter()
                .map(|c| Task::done(Message::Transition(c.done()))),
        )
    }

    /// Leaves the jam: the search text is only for finding something, so it does not stay.
    fn leave_jam(&mut self) {
        self.jam_open = false;
        self.jam_filters_open = false;
        self.jam_search_open = false;
        self.search_open = false;
        self.collection_query.clear();
        self.filter.q.clear();
        self.save_name = None;
    }

    fn close_overlays(&mut self) {
        self.manual_filter_review = None;
        self.playlist_tracks_open = false;
        self.playlist_tracks_query.clear();
        self.playlist_edit = None;
        self.search_open = false;
        self.page_filters_open = false;
        self.import_report = None;
        self.draft = None;
        self.edit_track = None;
        self.edit = None;
        self.desc_expanded = false;
        self.leave_jam();
        self.now_playing = false;
        self.queue_open = false;
    }

    /// Shows a page: anything open over the page (the jam, a dialog, Now Playing) closes.
    fn show(&mut self, d: Dest) -> Vec<iad::transition::Command<Layout>> {
        self.dest = d;
        self.explicit_sort = false;
        self.collection_query.clear();
        self.detail_order = None;
        self.detail_descending = true;
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
        if self.dest == Dest::Recent
            || (self.dest == Dest::Tracks
                && self.collection_order() == weave_player_lib::core::CollectionOrder::Recent)
            || self.detail_order == Some(weave_player_lib::core::CollectionOrder::Recent)
            || (self.jam_open
                && self
                    .filter
                    .retained
                    .sort
                    .is_some_and(|s| s.field == weave_player_lib::core::SortField::LastPlayed))
        {
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

    /// Whether the page is a playlist that can be deleted: any but the default Favourites.
    pub fn can_delete_playlist(&self) -> bool {
        matches!(self.dest, Dest::Playlist(i)
            if self.lib.playlists.get(i).is_some_and(|p| p.name != backend::FAVOURITES))
    }

    pub fn manual_playlist(&self) -> Option<&data::Playlist> {
        if self.jam_open || self.queue_open || self.now_playing {
            return None;
        }
        match self.dest {
            Dest::Playlist(i) => self.lib.playlists.get(i).filter(|p| p.manual),
            _ => None,
        }
    }

    /// Whether the track is in the Favourites playlist.
    pub fn is_favourite(&self, id: usize) -> bool {
        self.lib
            .playlists
            .iter()
            .any(|p| p.name == backend::FAVOURITES && p.manual && p.tracks.contains(&id))
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

    /// Rebuilds the rows of the current page: its scope (the jam's filters, or the page's own), then
    /// the sort.
    pub fn refresh(&mut self) {
        self.query_error = None;
        let mut rows: Vec<Track> = if self.jam_open {
            // The jam page: the tracks the filters match.
            match self.backend.matching(&self.filter.search_spec()) {
                Ok(indices) => {
                    self.query_error = None;
                    indices
                        .iter()
                        .filter_map(|i| self.lib.tracks.get(*i).cloned())
                        .collect()
                }
                Err(error) => {
                    self.query_error = Some(error);
                    Vec::new()
                }
            }
        } else {
            match self.dest {
                Dest::Tracks => match self.backend.collection_tracks(
                    &self.collection_query,
                    self.selected_collection_order(),
                    self.collection_descending(),
                ) {
                    Ok(indices) => indices
                        .into_iter()
                        .filter_map(|i| self.lib.tracks.get(i).cloned())
                        .collect(),
                    Err(error) => {
                        self.query_error = Some(error);
                        Vec::new()
                    }
                },
                Dest::Rediscover => {
                    let mut spec = self.rediscover_filter.search_spec();
                    spec.not_played_for_days = Some(self.while_days);
                    spec.sort = Some(weave_player_lib::core::Sort {
                        field: self.rediscover_sort,
                        descending: true,
                    });
                    match self.backend.rediscover(&spec) {
                        Ok(indices) => {
                            self.query_error = None;
                            indices
                                .iter()
                                .filter_map(|i| self.lib.tracks.get(*i).cloned())
                                .collect()
                        }
                        Err(error) => {
                            self.query_error = Some(error);
                            Vec::new()
                        }
                    }
                }
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
        if !self.collection_query.trim().is_empty()
            && (self.jam_open
                || matches!(
                    self.dest,
                    Dest::Recent
                        | Dest::Rediscover
                        | Dest::Artist(_)
                        | Dest::Album(_)
                        | Dest::Playlist(_)
                ))
        {
            let candidates: Vec<_> = rows.iter().map(|t| t.id).collect();
            match self
                .backend
                .filter_indices(&candidates, &self.context_search_spec())
            {
                Ok(indices) => {
                    rows = indices
                        .into_iter()
                        .filter_map(|i| self.lib.tracks.get(i).cloned())
                        .collect()
                }
                Err(error) => {
                    self.query_error = Some(error);
                    rows.clear();
                }
            }
        }
        if !self.explicit_sort
            && let Some(order) = if self.jam_open {
                self.jam_order
            } else {
                self.detail_order
            }
            && (self.jam_open
                || matches!(
                    self.dest,
                    Dest::Artist(_) | Dest::Album(_) | Dest::Playlist(_)
                ))
        {
            match self.backend.order_indices(
                &rows.iter().map(|t| t.id).collect::<Vec<_>>(),
                order,
                self.collection_descending(),
            ) {
                Ok(indices) => {
                    rows = indices
                        .into_iter()
                        .filter_map(|i| self.lib.tracks.get(i).cloned())
                        .collect()
                }
                Err(error) => self.query_error = Some(error),
            }
        }
        if self.explicit_sort
            || (self.detail_order.is_none()
                && !(self.jam_open
                    && (self.jam_order.is_some() || self.filter.retained.sort.is_some()))
                && !matches!(
                    self.dest,
                    Dest::Tracks | Dest::Rediscover | Dest::Playlist(_)
                ))
        {
            self.sort.apply(&mut rows, |col, a, b| match col {
                Col::Title => cmp_str(&a.title.to_lowercase(), &b.title.to_lowercase()),
                Col::Artist => cmp_str(&a.artist.to_lowercase(), &b.artist.to_lowercase()),
                Col::Album => cmp_str(&a.album.to_lowercase(), &b.album.to_lowercase()),
                Col::Duration => a.secs.cmp(&b.secs),
            });
        }
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
        if self.has_playback_sort() {
            let snapshot = self.backend.shuffle(false);
            self.apply(snapshot);
            return false;
        }
        match self.page_shuffle() {
            Some(on) => {
                let s = self.backend.shuffle(on);
                self.apply(s);
                on
            }
            None => false,
        }
    }

    pub fn has_playback_sort(&self) -> bool {
        if self.jam_open {
            self.jam_order.is_some()
        } else if let Some(index) = self.collection_order_index() {
            self.collection_rule_order[index] && self.collection_orders[index].is_some()
        } else {
            self.detail_order.is_some()
        }
    }

    fn clear_playback_sort(&mut self) {
        if self.jam_open {
            self.jam_order = None;
        } else if let Some(index) = self.collection_order_index() {
            self.collection_rule_order[index] = false;
            self.collection_orders[index] = None;
        } else {
            self.detail_order = None;
        }
        self.refresh();
    }

    fn play(&mut self, ids: Vec<usize>, start: usize) {
        let session = self.backend.play(&ids, start);
        self.apply(session);
    }

    fn commit_manual_filter(&mut self, review: ManualFilterReview) {
        match self.backend.update_manual_playlist_filter(
            &review.name,
            &review.filter,
            &review.expected,
            &review.removed,
        ) {
            Ok(()) => {
                self.draft = None;
                self.reload_library();
                self.refresh();
            }
            Err(error) => self.say(format!("Cannot save the filter: {error}")),
        }
    }

    fn playlist_filter_draft_mut(&mut self) -> Option<&mut Filter> {
        if self.save_manual && self.save_name.is_some() {
            Some(&mut self.creation_filter)
        } else {
            self.draft.as_mut()
        }
    }

    fn page_filter_mut(&mut self) -> &mut Filter {
        if self.dest == Dest::AboutMe {
            &mut self.analytics_filter
        } else {
            &mut self.rediscover_filter
        }
    }
    pub fn matching<'a>(&'a self, filter: &Filter) -> Vec<&'a Track> {
        self.backend
            .matching(&filter.search_spec())
            .unwrap_or_default()
            .iter()
            .filter_map(|i| self.lib.tracks.get(*i))
            .collect()
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
            Message::SpectrumTick => {
                self.spectrum = weave_player_lib::core::spectrum::bands();
                Vec::new()
            }
            Message::Window(a) => {
                return iced::window::oldest().and_then(move |id| a.perform(id));
            }
            Message::Go(d) => {
                self.history.go(Place::at(d));
                let cmds = self.show(d);
                if d == Dest::AboutMe {
                    let task = self.refresh_analytics();
                    return Task::batch([self.dispatch(cmds), task]);
                }
                cmds
            }
            Message::Open(d) => {
                let here = Place { dest: self.dest };
                self.history.replace(here);
                self.history.open(Place::at(d));
                let cmds = self.show(d);
                if d == Dest::AboutMe {
                    let task = self.refresh_analytics();
                    return Task::batch([self.dispatch(cmds), task]);
                }
                cmds
            }
            Message::AnalyticsLoaded(request, result) => {
                if request == self.analytics_request {
                    self.analytics_loading = false;
                    self.analytics_result = Some(*result);
                }
                Vec::new()
            }
            Message::CollectionQuery(query) => {
                self.collection_query = query;
                self.refresh();
                Vec::new()
            }
            Message::CollectionOrder(order) => {
                if self.playlist_edit.is_some() {
                    return Task::none();
                }
                let deselect = self.selected_collection_order() == Some(order)
                    && !self.collection_descending();
                let descending = if self.selected_collection_order() == Some(order) {
                    !self.collection_descending()
                } else {
                    true
                };
                if self.jam_open {
                    self.jam_order = (!deselect).then_some(order);
                    self.jam_descending = descending;
                } else if let Some(index) = self.collection_order_index() {
                    self.collection_orders[index] = (!deselect).then_some(order);
                    self.collection_descending[index] = descending;
                    self.collection_rule_order[index] = !deselect;
                } else if matches!(
                    self.dest,
                    Dest::Artist(_) | Dest::Album(_) | Dest::Playlist(_)
                ) {
                    self.detail_order = (!deselect).then_some(order);
                    self.detail_descending = descending;
                } else {
                    return Task::none();
                }
                self.explicit_sort = false;
                self.refresh();
                Vec::new()
            }
            Message::ExploreSection(i) => {
                if let Some(open) = self.explore_open.get_mut(i) {
                    *open = !*open;
                }
                Vec::new()
            }
            Message::ArtistSection(index) => {
                if let Dest::Artist(artist) = self.dest
                    && let Some((name, _)) = self.lib.artists().get(artist)
                    && let Some(open) = self
                        .artist_open
                        .entry(name.clone())
                        .or_insert([true; 3])
                        .get_mut(index)
                {
                    *open = !*open;
                }
                Vec::new()
            }
            Message::PageFilters(open) => {
                if open && !self.page_filters_open {
                    self.prepare_filter_editor();
                }
                self.page_filters_open = open;
                Vec::new()
            }
            Message::PageTag(tag) => {
                toggle(&mut self.page_filter_mut().tags, tag);
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::PageArtist(artist) => {
                toggle(&mut self.page_filter_mut().artists, artist);
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::PageAlbum(album) => {
                toggle(&mut self.page_filter_mut().albums, album);
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::PageBpm(bpm) => {
                self.page_filter_mut().set_bpm(bpm);
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::PageRecent(on) => {
                self.page_filter_mut().imported_within_days = on.then_some(7);
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::Exclusions(text) => {
                self.filter.exclusion_input = text;
                self.refresh();
                Vec::new()
            }
            Message::DraftExclusions(text) => {
                if let Some(f) = self.playlist_filter_draft_mut() {
                    f.exclusion_input = text;
                }
                Vec::new()
            }
            Message::PageExclusions(text) => {
                self.page_filter_mut().exclusion_input = text;
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::PageRating(rating) => {
                self.page_filter_mut().retained.rating_min = (rating > 0).then_some(rating);
                self.refresh();
                if self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::RediscoverDays(text) => {
                if let Ok(days) = text.parse() {
                    self.while_days = days;
                }
                self.rediscover_days = text;
                self.refresh();
                Vec::new()
            }
            Message::RediscoverSort(sort) => {
                self.rediscover_sort = sort;
                self.explicit_sort = false;
                self.refresh();
                Vec::new()
            }
            Message::AnalyticsPeriod(period) => {
                self.analytics_period = period;
                return self.refresh_analytics();
            }
            Message::AnalyticsDateText(text) => {
                self.analytics_range = iad::primitives::range::DateRange::parse(&text);
                self.analytics_date_text = text;
                return self.refresh_analytics();
            }
            Message::AnalyticsDatePick(range) => {
                self.analytics_range = range;
                self.analytics_date_text = range.text();
                return self.refresh_analytics();
            }
            Message::AnalyticsDateView(date) => {
                self.analytics_date_view = date;
                Vec::new()
            }
            Message::SearchOpen(open) => {
                self.search_open = open && self.can_search_page();
                if !self.search_open {
                    self.collection_query.clear();
                }
                self.refresh();
                Vec::new()
            }
            Message::SearchText(text) => {
                self.collection_query = text;
                self.refresh();
                Vec::new()
            }
            Message::JamSearchOpen(on) => {
                // Compatibility with the existing development scenarios: use inline search.
                self.jam_search_open = on;
                self.search_open = on;
                if !on {
                    // Closing the bar drops the text: it was only there to find something.
                    self.filter.q.clear();
                    self.refresh();
                }
                Vec::new()
            }
            Message::JamArtistToggle(a) => {
                toggle(&mut self.filter.artists, a);
                self.refresh();
                Vec::new()
            }
            Message::JamAlbumToggle(a) => {
                toggle(&mut self.filter.albums, a);
                self.refresh();
                Vec::new()
            }
            Message::ArtistQuery(t) => {
                self.artist_query = t;
                Vec::new()
            }
            Message::AlbumQuery(t) => {
                self.album_query = t;
                Vec::new()
            }
            Message::DraftArtistToggle(a) => {
                if let Some(d) = self.playlist_filter_draft_mut() {
                    toggle(&mut d.artists, a);
                }
                Vec::new()
            }
            Message::DraftAlbumToggle(a) => {
                if let Some(d) = self.playlist_filter_draft_mut() {
                    toggle(&mut d.albums, a);
                }
                Vec::new()
            }
            Message::JamRecent(on) => {
                self.filter.imported_within_days = on.then_some(7);
                self.refresh();
                Vec::new()
            }
            Message::DraftRecent(on) => {
                if let Some(d) = self.playlist_filter_draft_mut() {
                    d.imported_within_days = on.then_some(7);
                }
                Vec::new()
            }
            Message::JamQuery(t) => {
                self.filter.q = t;
                self.refresh();
                Vec::new()
            }
            Message::ToggleJam => {
                self.playlist_edit = None;
                self.search_open = false;
                self.collection_query.clear();
                if self.jam_open {
                    // On desktop, unselecting a header button returns to Home.
                    if !self.is_narrow() {
                        return self.update(Message::Go(Dest::Home));
                    }
                    self.leave_jam();
                } else {
                    self.jam_open = true;
                    self.now_playing = false;
                }
                self.refresh();
                Vec::new()
            }
            Message::ToggleExplore => {
                if self.dest == Dest::Explore && !self.jam_open {
                    return self.update(Message::Go(Dest::Home));
                }
                return self.update(Message::Go(Dest::Explore));
            }
            Message::Themes(open) => {
                self.themes_open = open;
                self.sheet = None;
                Vec::new()
            }
            Message::ImportTheme => {
                if self.theme_import_pending {
                    return Task::none();
                }
                self.theme_import_pending = true;
                return Task::perform(appearance::choose(), Message::ThemeChosen);
            }
            Message::ThemeChosen(path) => {
                self.theme_import_pending = false;
                if let Some(path) = path {
                    match self.themes.import(&path) {
                        Ok(key) => return self.update(Message::SelectTheme(Some(key))),
                        Err(error) => self.say(format!("Cannot import theme: {error}")),
                    }
                }
                Vec::new()
            }
            Message::SelectTheme(key) => {
                match self.themes.select(key) {
                    Ok(()) => self.apply_theme(),
                    Err(error) => self.say(format!("Cannot select theme: {error}")),
                }
                Vec::new()
            }
            Message::SetDensity(d) => {
                self.density = d;
                Vec::new()
            }
            Message::ThemeMode(mode) => {
                self.theme_mode = mode;
                Vec::new()
            }
            Message::SetShufflePref(pref) => {
                if pref == Some(true) {
                    self.clear_playback_sort();
                }
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
            Message::Import(mode) => {
                if self.import_in_progress || self.import_chooser_open {
                    return Task::none();
                }
                self.import_report = None;
                self.import_chooser_open = true;
                self.import_request += 1;
                let request = self.import_request;
                return Task::perform(import_picker::choose(mode), move |paths| {
                    Message::ImportChosen(request, paths)
                });
            }
            Message::ImportChosen(request, paths) => {
                if request != self.import_request || !self.import_chooser_open {
                    return Task::none();
                }
                self.import_chooser_open = false;
                return self.update(Message::Picked(paths));
            }
            Message::DismissImportReport => {
                self.import_report = None;
                Vec::new()
            }
            Message::Picked(paths) => {
                if self.import_in_progress || paths.is_empty() {
                    return Task::none();
                }
                let Some(db) = self.backend.db().map(|d| d.to_path_buf()) else {
                    self.say("Import needs a library database");
                    return Task::none();
                };
                self.import_in_progress = true;
                self.say("Importing…");
                return Task::perform(import(db, paths), Message::Imported);
            }
            Message::Imported(result) => {
                self.import_in_progress = false;
                match result {
                    Ok(report) => {
                        self.playlist_edit = None;
                        self.reload();
                        self.filter.imported_within_days = Some(7);
                        self.queue_open = false;
                        self.jam_open = true;
                        self.now_playing = false;
                        self.refresh();
                        self.say(import_picker::summary(&report));
                        self.import_report = (!report.issues.is_empty()).then_some(report);
                    }
                    Err(e) => {
                        self.say(format!("Import failed: {e}"));
                    }
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
                if self.themes_open {
                    return self.update(Message::Themes(false));
                }
                if self.manual_filter_review.is_some() {
                    return self.update(Message::CancelManualFilterReview);
                }
                if let Some(context) = self.active_filter_context() {
                    return self.update(self.filter_close(context));
                }
                if self.save_manual && self.save_name.is_some() {
                    return self.update(Message::CancelSaveJam);
                }
                if self.playlist_tracks_open {
                    return self.update(Message::PlaylistTracksOpen(false));
                }
                if self.fullscreen {
                    return self.update(Message::Fullscreen(false));
                }
                if self.search_open {
                    return self.update(Message::SearchOpen(false));
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
            Message::SelectTagGroup(target, group) => {
                let filter = match target {
                    FilterTarget::Draft => self.playlist_filter_draft_mut(),
                    FilterTarget::Page => Some(self.page_filter_mut()),
                    FilterTarget::Jam => Some(&mut self.filter),
                };
                let group = group.filter(|&(block, index)| {
                    filter
                        .as_ref()
                        .and_then(|f| f.retained.tag_blocks.get(block))
                        .and_then(|b| b.get(index))
                        .is_some_and(crate::ui_dialogs::editable_tag_group)
                });
                self.active_tag_group = group.map(|(block, index)| (target, block, index));
                return iced::widget::operation::focus(iced::widget::Id::new(
                    crate::ui_dialogs::TAG_QUERY_ID,
                ));
            }
            Message::Noop => Vec::new(),
            Message::ToggleInTagGroup(target, at, tag) => {
                let select = self.update(Message::SelectTagGroup(target, Some(at)));
                let toggle = self.update(Message::FilterTagToggle(target, tag));
                return Task::batch([select, toggle]);
            }
            Message::FilterTagToggle(target, tag) => {
                let active = self
                    .active_tag_group
                    .filter(|(context, _, _)| *context == target);
                let filter = match target {
                    FilterTarget::Draft => self.playlist_filter_draft_mut(),
                    FilterTarget::Page => Some(self.page_filter_mut()),
                    FilterTarget::Jam => Some(&mut self.filter),
                };
                let mut next_active = active;
                if let Some(filter) = filter {
                    if let Some((_, block, index)) = active
                        && let Some(group) = filter
                            .retained
                            .tag_blocks
                            .get_mut(block)
                            .and_then(|b| b.get_mut(index))
                        && crate::ui_dialogs::editable_tag_group(group)
                    {
                        let tags = if group.any.is_empty() {
                            &mut group.all
                        } else {
                            &mut group.any
                        };
                        toggle(tags, tag);
                        if tags.is_empty() {
                            filter.retained.tag_blocks[block].remove(index);
                            filter.retained.tag_blocks.retain(|b| !b.is_empty());
                            next_active = None;
                        }
                    } else if !filter.retained.tag_blocks.is_empty()
                        && filter.retained.tag_all.is_empty()
                        && filter.tags.is_empty()
                    {
                        // New selections belong beside the committed alternatives, not in
                        // tag_all, which would AND them with the entire OR block.
                        let index = filter.retained.tag_blocks[0].len();
                        filter.retained.tag_blocks[0].push(weave_player_lib::core::TagFilter {
                            all: vec![tag],
                            ..Default::default()
                        });
                        next_active = Some((target, 0, index));
                    } else {
                        toggle(&mut filter.tags, tag);
                        next_active = None;
                    }
                }
                if active.is_some() || next_active.is_some() {
                    self.active_tag_group = next_active;
                }
                if target != FilterTarget::Draft {
                    self.refresh();
                }
                if target == FilterTarget::Page && self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::CommitTagGroup(target) => {
                let editing = self
                    .active_tag_group
                    .is_some_and(|(context, _, _)| context == target);
                self.active_tag_group = None;
                let filter = match target {
                    FilterTarget::Draft => self.playlist_filter_draft_mut(),
                    FilterTarget::Page => Some(self.page_filter_mut()),
                    FilterTarget::Jam => Some(&mut self.filter),
                };
                if let Some(filter) = filter
                    && !editing
                    && !filter.tags.is_empty()
                {
                    if filter.retained.tag_blocks.is_empty() {
                        filter.retained.tag_blocks.push(Vec::new());
                    }
                    filter.retained.tag_all.clear();
                    filter.retained.tag_blocks[0].push(weave_player_lib::core::TagFilter {
                        all: std::mem::take(&mut filter.tags),
                        ..Default::default()
                    });
                }
                if target != FilterTarget::Draft {
                    self.refresh();
                }
                if target == FilterTarget::Page && self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                return iced::widget::operation::focus(iced::widget::Id::new(
                    crate::ui_dialogs::TAG_QUERY_ID,
                ));
            }
            Message::RemoveGroupedTag(target, block, index, tag) => {
                let mut active = self.active_tag_group;
                let filter = match target {
                    FilterTarget::Draft => self.playlist_filter_draft_mut(),
                    FilterTarget::Page => Some(self.page_filter_mut()),
                    FilterTarget::Jam => Some(&mut self.filter),
                };
                if let Some(filter) = filter
                    && let Some(group) = filter
                        .retained
                        .tag_blocks
                        .get_mut(block)
                        .and_then(|groups| groups.get_mut(index))
                    && crate::ui_dialogs::editable_tag_group(group)
                {
                    let tags = if group.any.is_empty() {
                        &mut group.all
                    } else {
                        &mut group.any
                    };
                    tags.retain(|name| *name != tag);
                    if tags.is_empty() {
                        filter.retained.tag_blocks[block].remove(index);
                        let removed_block = filter.retained.tag_blocks[block].is_empty();
                        if removed_block {
                            filter.retained.tag_blocks.remove(block);
                        }
                        active = active.and_then(|(context, active_block, active_index)| {
                            if context != target {
                                Some((context, active_block, active_index))
                            } else if (active_block, active_index) == (block, index) {
                                None
                            } else {
                                Some((
                                    context,
                                    active_block
                                        - usize::from(removed_block && active_block > block),
                                    active_index
                                        - usize::from(
                                            active_block == block && active_index > index,
                                        ),
                                ))
                            }
                        });
                    }
                }
                self.active_tag_group = active;
                if target != FilterTarget::Draft {
                    self.refresh();
                }
                if target == FilterTarget::Page && self.dest == Dest::AboutMe {
                    return self.refresh_analytics();
                }
                Vec::new()
            }
            Message::ToggleTag(t) => {
                toggle(&mut self.filter.tags, t);
                self.refresh();
                Vec::new()
            }
            Message::ChipExpand(picker, expanded) => {
                self.chip_expanded[picker as usize] = expanded;
                Vec::new()
            }
            Message::TagBrowseToggle => {
                self.tag_browse = if self.tag_browse == TagBrowse::All {
                    TagBrowse::Categories
                } else {
                    TagBrowse::All
                };
                self.chip_expanded[ChipPicker::Tags as usize] = false;
                self.chip_expanded[ChipPicker::TrackTags as usize] = false;
                Vec::new()
            }
            Message::TagCategories => {
                self.tag_browse = TagBrowse::Categories;
                self.chip_expanded[ChipPicker::Tags as usize] = false;
                self.chip_expanded[ChipPicker::TrackTags as usize] = false;
                Vec::new()
            }
            Message::TagCategory(category) => {
                // A category-name search (notably "Custom") must not open an empty child list.
                // Keep a tag search such as "sensual" when it still matches inside the category.
                if !self.tag_query.trim().is_empty()
                    && matches!(self.backend.tag_choices(&self.tag_query, Some(category.as_deref().unwrap_or(""))), Ok(tags) if tags.is_empty())
                {
                    self.tag_query.clear();
                }
                self.tag_browse = TagBrowse::Category(category);
                self.chip_expanded[ChipPicker::Tags as usize] = false;
                self.chip_expanded[ChipPicker::TrackTags as usize] = false;
                Vec::new()
            }
            Message::TagQuery(q) => {
                self.tag_query = q;
                Vec::new()
            }
            Message::Bpm(r) => {
                self.filter.set_bpm(r);
                self.refresh();
                Vec::new()
            }
            Message::ClearTagFilters => {
                self.filter.imported_within_days = None;
                self.filter.tags.clear();
                self.filter.set_bpm(data::BPM);
                self.refresh();
                Vec::new()
            }
            Message::Surprise => {
                // Replaces the tags and the tempo, keeps the search; never a combination that
                // matches nothing.
                let names = self.lib.tag_names();
                for _ in 0..30 {
                    let mut f = Filter {
                        tags: Vec::new(),
                        bpm: data::BPM,
                        ..self.filter.clone()
                    };
                    f.set_bpm(data::BPM);
                    let picks = 1 + (self.roll() % 2) as usize;
                    for _ in 0..picks {
                        let t = names[(self.roll() % names.len() as u64) as usize].clone();
                        if !f.tags.contains(&t) {
                            f.tags.push(t);
                        }
                    }
                    if self.matching(&f).len() >= 3 {
                        self.filter = f;
                        break;
                    }
                }
                self.refresh();
                Vec::new()
            }
            Message::StartJam => {
                let ids: Vec<usize> = self.rows.iter().map(|t| t.id).collect();
                let ordered = self.has_playback_sort();
                if !ids.is_empty() {
                    let start = if ordered {
                        0
                    } else {
                        (self.roll() % ids.len() as u64) as usize
                    };
                    let snapshot = self.backend.shuffle(!ordered);
                    self.apply(snapshot);
                    self.play(ids, start);
                }
                self.jam_open = false;
                Vec::new()
            }
            Message::Sort(c) => {
                self.explicit_sort = true;
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
            Message::NewManualPlaylist => {
                self.creation_filter = Filter::default();
                self.chip_expanded = [false; 4];
                self.tag_browse = TagBrowse::Categories;
                self.tag_query.clear();
                self.artist_query.clear();
                self.album_query.clear();
                self.save_manual = true;
                self.save_name = Some(self.new_playlist_name());
                Vec::new()
            }
            Message::PlaylistTracksOpen(open) => {
                self.playlist_tracks_open =
                    open && self.playlist_edit.is_none() && self.manual_playlist().is_some();
                self.playlist_tracks_query.clear();
                Vec::new()
            }
            Message::PlaylistTracksQuery(query) => {
                self.playlist_tracks_query = query;
                Vec::new()
            }
            Message::PlaylistTrackToggle(id) => {
                if self.playlist_edit.is_none()
                    && let Some(playlist) = self.manual_playlist()
                    && let Some(track) = self.lib.tracks.get(id)
                {
                    let name = playlist.name.clone();
                    let present = playlist.tracks.contains(&id);
                    if !present {
                        match self.backend.playlist_add_candidates(&name, "") {
                            Ok(ids) if ids.contains(&id) => {}
                            Ok(_) => {
                                self.say("This track does not match the playlist rules");
                                return Task::none();
                            }
                            Err(error) => {
                                self.say(format!("Cannot check playlist rules: {error}"));
                                return Task::none();
                            }
                        }
                    }
                    match self
                        .backend
                        .set_playlist_track(&name, &track.slug, !present)
                    {
                        Ok(()) => {
                            if self.reload_library() {
                                if let Some(index) =
                                    self.lib.playlists.iter().position(|p| p.name == name)
                                {
                                    self.dest = Dest::Playlist(index);
                                }
                                self.refresh();
                            }
                        }
                        Err(error) => self.say(format!("Cannot edit playlist tracks: {error}")),
                    }
                }
                Vec::new()
            }
            Message::SaveJamAsPlaylist => {
                self.save_manual = false;
                if !self.filter.is_saveable() {
                    self.say("Add a tag, tempo, artist or album to save a playlist");
                } else if self.rows.is_empty() {
                    self.say("Nothing matches these filters yet");
                } else {
                    self.save_name = Some(self.new_playlist_name());
                }
                Vec::new()
            }
            Message::SaveNameInput(t) => {
                if self.save_name.is_some() {
                    self.save_name = Some(t);
                }
                Vec::new()
            }
            Message::CancelSaveJam => {
                self.save_name = None;
                Vec::new()
            }
            Message::ConfirmSaveJam => {
                let Some(name) = self.save_name.clone().map(|n| n.trim().to_string()) else {
                    return Task::none();
                };
                if name.is_empty() {
                    self.say("Give the playlist a name");
                } else if self.name_is_taken(&name) {
                    self.say(format!("There is already a playlist called {name}"));
                } else if !self.save_manual && !self.filter.is_saveable() {
                    self.save_name = None;
                    self.say("Add a tag, tempo, artist or album to save a playlist");
                } else {
                    let saved_with = self.filter.clone();
                    let result = if self.save_manual {
                        self.backend
                            .create_manual_playlist(&name, &self.creation_filter)
                    } else {
                        self.backend.create_dynamic_playlist(&name, &saved_with)
                    };
                    match result {
                        Ok(()) => {
                            self.save_name = None;
                            if self.reload_library()
                                && let Some(i) =
                                    self.lib.playlists.iter().position(|p| p.name == name)
                            {
                                if self.save_manual {
                                    self.history.replace(Place { dest: self.dest });
                                    self.history.open(Place::at(Dest::Playlist(i)));
                                    self.show(Dest::Playlist(i));
                                } else {
                                    self.jam_saved = Some((i, saved_with.clone()));
                                }
                            }
                            self.say(if self.save_manual {
                                format!("Created {name}")
                            } else if saved_with.q.trim().is_empty() {
                                format!("Saved as {name}")
                            } else {
                                format!(
                                    "Saved as {name}. The search text is not saved; tags, tempo, artists and albums are."
                                )
                            });
                        }
                        Err(e) => self.say(format!("Cannot save the playlist: {e}")),
                    }
                }
                Vec::new()
            }
            Message::AskDeletePlaylist => {
                if self.can_delete_playlist()
                    && let Dest::Playlist(i) = self.dest
                {
                    self.delete_playlist = self.lib.playlists.get(i).map(|p| p.name.clone());
                }
                Vec::new()
            }
            Message::CancelDeletePlaylist => {
                self.delete_playlist = None;
                Vec::new()
            }
            Message::ConfirmDeletePlaylist => {
                let Some(name) = self.delete_playlist.take() else {
                    return Task::none();
                };
                if name == backend::FAVOURITES {
                    self.say("The Favourites playlist cannot be deleted");
                    return Task::none();
                }
                match self.backend.delete_playlist(&name) {
                    Ok(_) => {
                        // The page was the playlist, whose place in the list is gone.
                        self.jam_saved = None;
                        self.dest = Dest::Playlists;
                        self.history = History::new(Place::at(Dest::Playlists));
                        self.reload_library();
                        self.say(format!("Deleted {name}"));
                    }
                    Err(e) => self.say(format!("Cannot delete the playlist: {e}")),
                }
                Vec::new()
            }
            Message::JamFilters(on) => {
                if on && !self.jam_filters_open {
                    self.prepare_filter_editor();
                }
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
                self.chip_expanded[ChipPicker::TrackTags as usize] = false;
                self.tag_browse = TagBrowse::All;
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
            Message::Back => {
                if let Some(p) = self.history.back().cloned() {
                    self.dest = p.dest;
                    self.close_overlays();
                    self.refresh();
                    if self.dest == Dest::AboutMe {
                        return self.refresh_analytics();
                    }
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
                self.prepare_filter_editor();
                if let Dest::Playlist(i) = self.dest {
                    self.draft = self
                        .lib
                        .playlists
                        .get(i)
                        .map(|p| p.filter.clone().unwrap_or_default());
                }
                Vec::new()
            }
            Message::DraftToggle(t) => {
                if let Some(d) = self.playlist_filter_draft_mut() {
                    toggle(&mut d.tags, t);
                }
                Vec::new()
            }
            Message::DraftBpm(r) => {
                if let Some(d) = self.playlist_filter_draft_mut() {
                    d.set_bpm(r);
                }
                Vec::new()
            }
            Message::SaveFilter => {
                if let (Dest::Playlist(i), Some(filter)) = (self.dest, self.draft.clone())
                    && let Some(playlist) = self.lib.playlists.get(i)
                {
                    let name = playlist.name.clone();
                    let tags_removed = playlist.filter.as_ref().is_some_and(|old| {
                        old.tags.iter().any(|tag| !filter.tags.contains(tag))
                            || old
                                .to_spec()
                                .tag_exclude
                                .iter()
                                .any(|tag| !filter.to_spec().tag_exclude.contains(tag))
                    });
                    if playlist.manual {
                        match self.backend.preview_manual_playlist_filter(&name, &filter) {
                            Ok((expected, removed)) => {
                                let review = ManualFilterReview {
                                    name,
                                    filter,
                                    expected,
                                    removed,
                                };
                                if review.removed.is_empty() && !tags_removed {
                                    self.commit_manual_filter(review);
                                } else {
                                    self.manual_filter_review = Some(review);
                                }
                            }
                            Err(error) => self.say(format!("Cannot review the filter: {error}")),
                        }
                    } else {
                        match self.backend.set_playlist_filter(&name, &filter) {
                            Ok(()) => {
                                self.draft = None;
                                self.reload_library();
                            }
                            Err(error) => self.say(format!("Cannot save the filter: {error}")),
                        }
                    }
                }
                self.refresh();
                Vec::new()
            }
            Message::ConfirmManualFilter => {
                if let Some(review) = self.manual_filter_review.take() {
                    self.commit_manual_filter(review);
                }
                Vec::new()
            }
            Message::CancelManualFilterReview => {
                self.manual_filter_review = None;
                Vec::new()
            }
            Message::CancelFilter => {
                self.draft = None;
                self.manual_filter_review = None;
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
                if !self.player.shuffle {
                    self.clear_playback_sort();
                }
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
            Message::PlaylistOrder(event) => {
                match event {
                    ReorderEvent::Edit => {
                        if self.jam_open {
                            return Task::none();
                        }
                        let Dest::Playlist(index) = self.dest else {
                            return Task::none();
                        };
                        let Some(playlist) = self.lib.playlists.get(index).filter(|p| p.manual)
                        else {
                            return Task::none();
                        };
                        let name = playlist.name.clone();
                        match self.backend.playlist_order(&name) {
                            Ok(tracks) => {
                                if tracks.len() < 2 {
                                    return Task::none();
                                }
                                let mut order = ReorderState::new(tracks);
                                order.edit();
                                self.playlist_edit = Some(PlaylistOrderEdit { name, order });
                                self.search_open = false;
                                self.collection_query.clear();
                                self.detail_order = None;
                                self.explicit_sort = false;
                                self.refresh();
                            }
                            Err(error) => self.say(format!("Cannot edit playlist order: {error}")),
                        }
                    }
                    ReorderEvent::Reorder { from, to } => {
                        if let Some(edit) = &mut self.playlist_edit {
                            edit.order.reorder(from, to);
                        }
                    }
                    ReorderEvent::Cancel => {
                        self.playlist_edit = None;
                        self.refresh();
                    }
                    ReorderEvent::Submit => {
                        if let Some(edit) = &self.playlist_edit {
                            let name = edit.name.clone();
                            match self.backend.reorder_playlist(
                                &name,
                                edit.order.committed(),
                                edit.order.items(),
                            ) {
                                Ok(()) => {
                                    self.playlist_edit = None;
                                    if self.reload_library() {
                                        if let Some(index) =
                                            self.lib.playlists.iter().position(|p| p.name == name)
                                        {
                                            self.dest = Dest::Playlist(index);
                                        }
                                        self.refresh();
                                        self.say("Playlist order saved");
                                    }
                                }
                                Err(error) => {
                                    self.say(format!("Cannot save playlist order: {error}"))
                                }
                            }
                        }
                    }
                }
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
            Message::SystemTheme(settings) => {
                self.desktop_scheme = settings.scheme;
                Vec::new()
            }
            Message::Poll => {
                if self.dest == Dest::AboutMe
                    && !self.analytics_loading
                    && self.last_analytics.elapsed() > Duration::from_secs(5)
                {
                    return self.refresh_analytics();
                }
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

    /// Whether a playlist already has this name (case-insensitive).
    pub fn name_is_taken(&self, name: &str) -> bool {
        self.lib
            .playlists
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name.trim()))
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
        for (names, none) in [
            (&self.filter.artists, "Unknown artist"),
            (&self.filter.albums, "No album"),
        ] {
            if !names.is_empty() {
                let shown: Vec<&str> = names
                    .iter()
                    .map(|n| if n == data::UNKNOWN { none } else { n.as_str() })
                    .collect();
                parts.push(shown.join(" or "));
            }
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
            iad::utils::follow_theme().map(Message::SystemTheme),
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
        if self.radial_visualizer_visible()
            && (self.player.playing || self.spectrum.iter().any(|l| *l > 0.0))
        {
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

    fn effective_scheme(&self) -> iad::Scheme {
        self.theme_mode.unwrap_or(self.desktop_scheme)
    }

    fn theme(&self) -> iad::Theme {
        let theme = appearance::theme(self.effective_scheme(), &self.theme_config);
        if self.theme_config.bool("player", "ambient") == Some(true) {
            if let Some(track) = self.now() {
                let color = iad::color::from_index(track.id);
                return theme.with_tint(Some(iced::Color { a: 0.16, ..color }));
            }
        }
        theme
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

/// Indexes `paths` on a thread of its own, so the window keeps drawing.
async fn import(
    db: std::path::PathBuf,
    paths: Vec<std::path::PathBuf>,
) -> Result<weave_player_lib::core::ImportReport, String> {
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
    fn desktop_events_rebuild_adaptive_palette_and_tag_colors_without_resetting_state() {
        let mut a = app();
        let document = appearance::ThemeDocument::parse(appearance::EXAMPLES[0]).unwrap();
        a.theme_config = document.config;
        let queue = a.player.queue.clone();
        let selected = a.themes.selected.clone();
        let event = |scheme| {
            Message::SystemTheme(iad::utils::ThemeSettings {
                scheme,
                icon_theme: "hicolor".into(),
            })
        };
        let _ = a.update(event(iad::Scheme::Light));
        let light = a.theme();
        let light_tag = a.category_style(Some("genre")).color;
        assert_eq!(light.scheme, iad::Scheme::Light);
        let _ = a.update(event(iad::Scheme::Dark));
        let dark = a.theme();
        assert_eq!(dark.scheme, iad::Scheme::Dark);
        assert_ne!(dark.semantic.window.bg, light.semantic.window.bg);
        assert_ne!(a.category_style(Some("genre")).color, light_tag);
        let _ = a.update(event(iad::Scheme::Light));
        assert_eq!(a.theme().semantic.window.bg, light.semantic.window.bg);
        let _ = a.update(Message::ThemeMode(Some(iad::Scheme::Dark)));
        assert_eq!(a.theme().scheme, iad::Scheme::Dark);
        let _ = a.update(event(iad::Scheme::Light));
        assert_eq!(a.theme().semantic.window.bg, dark.semantic.window.bg);
        let _ = a.update(Message::ThemeMode(None));
        assert_eq!(a.theme().semantic.window.bg, light.semantic.window.bg);
        assert_eq!(a.themes.selected, selected);
        assert_eq!(a.player.queue, queue);
    }

    #[test]
    fn clicking_theme_mode_icons_emits_real_widget_messages_and_switches_the_palette() {
        use iced::advanced::{layout, renderer::Headless, widget::Tree};
        use iced::{Font, Pixels, Point, Size, mouse};
        fn leaves(layout: layout::Layout<'_>, size: f32, points: &mut Vec<Point>) {
            let children = layout.children().collect::<Vec<_>>();
            let bounds = layout.bounds();
            if children.is_empty()
                && (bounds.width - size).abs() < 0.01
                && (bounds.height - size).abs() < 0.01
            {
                points.push(bounds.center());
            }
            for child in children {
                leaves(child, size, points);
            }
        }
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.67),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut a = app();
        a.theme_config = appearance::config(appearance::EXAMPLES[0]);
        a.desktop_scheme = iad::Scheme::Dark;
        assert_eq!(a.theme_mode, None);
        let original_dark = a.theme().semantic.window.bg;
        for (index, expected) in [
            (1, Some(iad::Scheme::Light)),
            (2, Some(iad::Scheme::Dark)),
            (0, None),
        ] {
            let messages = {
                let mut element = a.theme_mode_row();
                let mut tree = Tree::new(&element);
                let size = Size::new(480.0, 100.0);
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                let mut points = Vec::new();
                leaves(
                    layout::Layout::new(&node),
                    iad::Metrics::desktop().icon_size,
                    &mut points,
                );
                assert_eq!(
                    points.len(),
                    3,
                    "system, sun and moon icons must all be clickable"
                );
                let cursor = mouse::Cursor::Available(points[index]);
                let mut messages = Vec::new();
                for event in [
                    iced::Event::Mouse(mouse::Event::CursorMoved {
                        position: points[index],
                    }),
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ] {
                    let mut shell = iced::advanced::Shell::new(&mut messages);
                    element.as_widget_mut().update(
                        &mut tree,
                        &event,
                        layout::Layout::new(&node),
                        cursor,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &iced::Rectangle::with_size(size),
                    );
                }
                messages
            };
            assert!(
                messages
                    .iter()
                    .any(|message| matches!(message,Message::ThemeMode(mode) if *mode == expected))
            );
            for message in messages {
                let _ = a.update(message);
            }
            assert_eq!(a.theme_mode, expected);
            assert_eq!(a.theme().scheme, expected.unwrap_or(iad::Scheme::Dark));
            if expected == Some(iad::Scheme::Light) {
                assert_ne!(a.theme().semantic.window.bg, original_dark);
            }
        }
        let _ = a.update(Message::SystemTheme(iad::utils::ThemeSettings {
            scheme: iad::Scheme::Light,
            icon_theme: "hicolor".into(),
        }));
        assert_eq!(a.theme().scheme, iad::Scheme::Light);
    }

    #[test]
    fn explicit_theme_mode_overrides_appearance_and_keeps_theme_density_and_queue() {
        let mut a = app();
        a.desktop_scheme = iad::Scheme::Light;
        let queue = a.player.queue.clone();
        let selected = a.themes.selected.clone();
        let _ = a.update(Message::SetDensity(DensityPref::Compact));
        assert_eq!(a.theme_mode, None);
        assert_eq!(a.theme().scheme, iad::Scheme::Light);
        let _ = a.update(Message::ThemeMode(Some(iad::Scheme::Dark)));
        assert_eq!(a.theme().scheme, iad::Scheme::Dark);
        let _ = a.update(Message::SystemTheme(iad::utils::ThemeSettings {
            scheme: iad::Scheme::Light,
            icon_theme: "hicolor".into(),
        }));
        assert_eq!(a.theme().scheme, iad::Scheme::Dark);
        let _ = a.update(Message::ThemeMode(None));
        assert_eq!(a.theme().scheme, iad::Scheme::Light);
        assert_eq!(a.desktop_scheme, iad::Scheme::Light);
        assert_eq!(a.themes.selected, selected);
        assert_eq!(a.player.queue, queue);
        assert!(a.compact());
    }

    #[test]
    fn theme_presets_render_pages_and_filters_without_changing_library_state() {
        use iced::advanced::{
            layout,
            renderer::{Headless, Style},
            widget::Tree,
        };
        use iced::{Font, Pixels, Point, Rectangle, Size, mouse};
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.67),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut a = app();
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        for name in ["After hours", "Velvet grooves", "Summer crush"] {
            a.backend.create_dynamic_playlist(name, &a.filter).unwrap();
        }
        assert!(a.reload_library());
        a.player.queue = vec![0];
        a.player.current = Some(0);
        // Synthetic levels only in this rendering fixture; production reads the audio tap.
        a.spectrum =
            std::array::from_fn(|index| ((index as f32 * 0.7).sin() * 0.35 + 0.45).clamp(0.0, 1.0));
        a.themes.themes = appearance::EXAMPLES
            .into_iter()
            .map(|text| appearance::ThemeDocument::parse(text).unwrap())
            .collect();
        let original_filter = a.filter.clone();
        let original_rows = a.rows.clone();
        for text in appearance::EXAMPLES {
            let document = appearance::ThemeDocument::parse(text).unwrap();
            for scheme in [iad::Scheme::Light, iad::Scheme::Dark] {
                a.desktop_scheme = scheme;
                a.theme_config = document.config.clone();
                a.themes.selected = Some(document.key.clone());
                assert_eq!(a.filter, original_filter);
                assert_eq!(a.rows, original_rows);
                assert_eq!(
                    a.tag_style("genre:bachata").color,
                    a.category_style(Some("genre")).color
                );
                for (scene, width, height) in [
                    ("app", 1000, 720),
                    ("home", 1000, 720),
                    ("home-narrow", 390, 720),
                    ("filters", 560, 520),
                    ("themes", 480, 560),
                    ("themes-sheet", 390, 844),
                    ("playlist-narrow", 390, 844),
                    ("now-playing", 700, 780),
                    ("now-playing-narrow", 390, 844),
                    ("filter-jam-wide", 1000, 720),
                    ("filter-jam-narrow", 390, 844),
                    ("filter-page-wide", 1000, 720),
                    ("filter-page-narrow", 390, 844),
                    ("filter-playlist-wide", 1000, 720),
                    ("filter-playlist-narrow", 390, 844),
                ] {
                    a.jam_open = scene.starts_with("filter-jam-");
                    a.jam_filters_open = a.jam_open;
                    a.page_filters_open = scene.starts_with("filter-page-");
                    a.draft = scene
                        .starts_with("filter-playlist-")
                        .then(|| a.filter.clone());
                    a.dest = if scene.starts_with("filter-page-") {
                        Dest::Rediscover
                    } else if scene == "playlist-narrow" || scene.starts_with("filter-playlist-") {
                        Dest::Playlist(0)
                    } else {
                        Dest::Home
                    };
                    let _ = a.update(Message::Resized(width as f32, height as f32));
                    let _ = a.update(Message::Tick(
                        Instant::now() + std::time::Duration::from_secs(2),
                    ));
                    a.now_playing = scene.starts_with("now-playing");
                    a.themes_open = scene == "themes-sheet";
                    let theme = a.theme();
                    let mut element = if scene == "filters" {
                        crate::components::responsive_dialog::panel(
                            a.filter_editor(FilterTarget::Jam),
                            false,
                            Size::new(width as f32, height as f32),
                            0.0,
                        )
                    } else if scene == "themes" {
                        a.themes_form()
                    } else if scene == "themes-sheet" || scene.starts_with("filter-") {
                        a.view()
                    } else if scene.starts_with("now-playing") {
                        a.now_playing_view()
                    } else if scene == "app" {
                        a.view()
                    } else {
                        a.page()
                    };
                    let mut tree = Tree::new(&element);
                    let size = Size::new(width as f32, height as f32);
                    let node = element.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, size),
                    );
                    assert!(node.size().width <= size.width && node.size().height <= size.height);
                    iced::advanced::Renderer::reset(
                        &mut renderer,
                        Rectangle::new(Point::ORIGIN, size),
                    );
                    let mut messages = Vec::new();
                    let mut shell = iced::advanced::Shell::new(&mut messages);
                    element.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now())),
                        layout::Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &Rectangle::new(Point::ORIGIN, size),
                    );
                    element.as_widget().draw(
                        &tree,
                        &mut renderer,
                        &theme,
                        &Style {
                            text_color: theme.semantic.window.fg,
                        },
                        layout::Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &Rectangle::new(Point::ORIGIN, size),
                    );
                    if let Some(dir) = std::env::var_os("WEAVE_THEME_REVIEW_DIR") {
                        let physical = Size::new(width, height);
                        let pixels = renderer.screenshot(physical, 1.0, theme.semantic.window.bg);
                        let path = std::path::PathBuf::from(dir).join(format!(
                            "{}-{}-{scene}.png",
                            document.key,
                            if scheme.is_dark() { "dark" } else { "light" }
                        ));
                        iad::dev::save_png(
                            &iced::window::Screenshot::new(pixels, physical, 1.0),
                            &path,
                        )
                        .unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn filter_contexts_share_responsive_presentation_and_keep_edit_semantics() {
        let mut a = app();
        let check = |a: &mut App, target| {
            assert_eq!(a.active_filter_context(), Some(target));
            for (width, narrow) in [(1000.0, false), (390.0, true)] {
                let _ = a.update(Message::Resized(width, 844.0));
                let _ = a.update(Message::Tick(Instant::now() + Duration::from_secs(2)));
                assert_eq!(a.dialog_open_for_test(), !narrow);
                assert_eq!(
                    a.active_filter_context(),
                    Some(target),
                    "resizing preserves context"
                );
                let _ = a.view();
            }
        };
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::JamFilters(true));
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        check(&mut a, FilterTarget::Jam);
        let live = a.filter.clone();
        let _ = a.update(Message::EscapeKey);
        assert!(a.active_filter_context().is_none());
        assert_eq!(a.filter, live, "closing a live editor keeps its changes");

        let _ = a.update(Message::Go(Dest::Rediscover));
        let _ = a.update(Message::PageFilters(true));
        let _ = a.update(Message::PageTag("practice".into()));
        check(&mut a, FilterTarget::Page);
        let live_page = a.rediscover_filter.clone();
        let _ = a.update(Message::EscapeKey);
        assert_eq!(a.rediscover_filter, live_page);
        let _ = a.update(Message::Go(Dest::AboutMe));
        let _ = a.update(Message::PageFilters(true));
        check(&mut a, FilterTarget::Page);
        let _ = a.update(Message::PageFilters(false));

        a.backend
            .create_dynamic_playlist("Filter context", &live)
            .unwrap();
        assert!(a.reload_library());
        let index = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == "Filter context")
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        let original = a.lib.playlists[index].filter.clone();
        let _ = a.update(Message::EditFilter);
        let _ = a.update(Message::DraftToggle("practice".into()));
        check(&mut a, FilterTarget::Draft);
        let _ = a.update(Message::EscapeKey);
        assert!(a.draft.is_none());
        assert_eq!(
            a.lib.playlists[index].filter, original,
            "cancel never persists draft changes"
        );
        let _ = a.update(Message::EditFilter);
        let _ = a.update(Message::DraftToggle("practice".into()));
        let _ = a.update(Message::SaveFilter);
        assert!(a.draft.is_none());
        assert!(
            a.lib
                .playlists
                .iter()
                .find(|p| p.name == "Filter context")
                .unwrap()
                .filter
                .as_ref()
                .unwrap()
                .tags
                .iter()
                .any(|tag| tag == "practice")
        );
    }

    #[test]
    fn themes_uses_a_dialog_or_sheet_and_keeps_density_separate() {
        let mut a = app();
        let root = import_picker::tests::temp();
        a.themes = appearance::ThemeLibrary::load(root.join("settings"));
        let _ = a.update(Message::SetDensity(DensityPref::Compact));
        let _ = a.update(Message::Resized(1000.0, 720.0));
        let _ = a.update(Message::Tick(Instant::now() + Duration::from_secs(2)));
        let _ = a.update(Message::Themes(true));
        assert!(a.themes_open && a.dialog_open_for_test());
        let _ = a.view();
        let source = root.join("theme.conf");
        std::fs::write(&source, appearance::EXAMPLES[0]).unwrap();
        let _ = a.update(Message::ThemeChosen(Some(source)));
        assert!(a.themes_open, "import must return to the chooser");
        assert!(a.compact(), "theme selection must keep compact density");
        assert!(a.themes.current().is_some());
        let _ = a.update(Message::SelectTheme(None));
        assert!(a.themes_open && a.themes.current().is_none());
        let _ = a.update(Message::Resized(390.0, 844.0));
        let _ = a.update(Message::Tick(Instant::now() + Duration::from_secs(2)));
        assert!(
            a.themes_open && !a.dialog_open_for_test(),
            "narrow uses a bottom sheet"
        );
        let _ = a.view();
        let _ = a.update(Message::EscapeKey);
        assert!(!a.themes_open);
        let _ = a.update(Message::OpenSheet(Some(Sheet::Main)));
        let _ = a.update(Message::SheetAction(Box::new(Message::Themes(true))));
        assert!(a.themes_open && a.sheet.is_none());
        let _ = a.update(Message::Themes(false));
        assert!(!a.themes_open && a.compact());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn theme_import_cancellation_failure_and_selection_preserve_the_music_state() {
        let mut a = app();
        let root = import_picker::tests::temp();
        a.themes = appearance::ThemeLibrary::load(root.join("themes-test"));
        let original = a.filter.clone();
        a.theme_import_pending = true;
        let _ = a.update(Message::ThemeChosen(None));
        assert!(!a.theme_import_pending);
        assert!(a.themes.themes.is_empty());
        let path = root.join("skin.conf");
        std::fs::write(&path, appearance::EXAMPLES[0]).unwrap();
        let _ = a.update(Message::ThemeChosen(Some(path.clone())));
        assert_eq!(a.themes.current().unwrap().name, "Neon Orbit");
        assert_eq!(a.theme_config.text("player", "artwork"), Some("disc"));
        assert_eq!(a.filter, original);
        std::fs::write(&path, "[theme]\nkey=../invalid\n").unwrap();
        let _ = a.update(Message::ThemeChosen(Some(path)));
        assert_eq!(a.themes.current().unwrap().name, "Neon Orbit");
        assert!(a.notice.as_ref().unwrap().0.contains("Cannot import theme"));
        let _ = a.update(Message::SelectTheme(None));
        assert_eq!(a.theme_config.text("player", "artwork"), Some("square"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn caret_selection_edits_only_the_active_tag_group() {
        let mut a = app();
        for tag in ["genre:bachata", "mood:romantic"] {
            let _ = a.update(Message::ToggleTag(tag.into()));
            let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        }
        for (index, tag) in [(0, "genre:zouk"), (1, "practice")] {
            let _ = a.update(Message::SelectTagGroup(FilterTarget::Jam, Some((0, index))));
            let _ = a.update(Message::FilterTagToggle(FilterTarget::Jam, tag.into()));
            assert_eq!(a.active_tag_group, Some((FilterTarget::Jam, 0, index)));
            assert!(a.filter.tags.is_empty());
        }
        assert_eq!(
            a.filter.retained.tag_blocks[0][0].all,
            ["genre:bachata", "genre:zouk"]
        );
        assert_eq!(
            a.filter.retained.tag_blocks[0][1].all,
            ["mood:romantic", "practice"]
        );
        let _ = a.update(Message::SelectTagGroup(FilterTarget::Jam, None));
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Jam,
            "genre:salsa".into(),
        ));
        assert!(a.filter.tags.is_empty());
        assert_eq!(a.filter.retained.tag_blocks[0].len(), 3);
        assert_eq!(a.filter.retained.tag_blocks[0][2].all, ["genre:salsa"]);
        assert_eq!(a.active_tag_group, Some((FilterTarget::Jam, 0, 2)));

        let _ = a.update(Message::SelectTagGroup(FilterTarget::Jam, Some((0, 0))));
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        assert_eq!(
            a.active_tag_group, None,
            "plus leaves the edited group for the new-group slot"
        );
        assert_eq!(
            a.filter.retained.tag_blocks[0].len(),
            3,
            "switching slots must not create another group"
        );
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        assert_eq!(a.filter.retained.tag_blocks[0].len(), 3);
        assert!(a.filter.tags.is_empty());
    }

    #[test]
    fn active_group_edits_stay_in_their_filter_context_and_parent_block() {
        let mut a = app();
        let mut filter = Filter::default();
        filter.retained.tag_blocks = vec![
            vec![weave_player_lib::core::TagFilter {
                any: vec!["genre:bachata".into()],
                ..Default::default()
            }],
            vec![weave_player_lib::core::TagFilter {
                any: vec!["mood:romantic".into()],
                ..Default::default()
            }],
        ];
        a.draft = Some(filter.clone());
        a.rediscover_filter = filter.clone();
        a.dest = Dest::Rediscover;
        let live = a.filter.clone();
        let _ = a.update(Message::SelectTagGroup(FilterTarget::Draft, Some((1, 0))));
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Draft,
            "practice".into(),
        ));
        assert_eq!(
            a.draft.as_ref().unwrap().retained.tag_blocks[1][0].any,
            ["mood:romantic", "practice"]
        );
        assert_eq!(
            a.draft.as_ref().unwrap().retained.tag_blocks[0],
            filter.retained.tag_blocks[0]
        );
        assert_eq!(a.rediscover_filter, filter);
        assert_eq!(a.filter, live);
        let _ = a.update(Message::SelectTagGroup(FilterTarget::Page, Some((0, 0))));
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Page,
            "genre:zouk".into(),
        ));
        assert_eq!(
            a.rediscover_filter.retained.tag_blocks[0][0].any,
            ["genre:bachata", "genre:zouk"]
        );
        assert_eq!(
            a.rediscover_filter.retained.tag_blocks[1],
            filter.retained.tag_blocks[1]
        );
        assert_eq!(a.filter, live);
        a.prepare_filter_editor();
        assert_eq!(a.active_tag_group, None);
    }

    #[test]
    fn removing_the_last_active_tag_does_not_leave_an_unrestricted_or_group() {
        let mut a = app();
        let _ = a.update(Message::ToggleTag("genre:bachata".into()));
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        let _ = a.update(Message::SelectTagGroup(FilterTarget::Jam, Some((0, 0))));
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Jam,
            "genre:bachata".into(),
        ));
        assert!(a.filter.retained.tag_blocks.is_empty());
        assert_eq!(a.active_tag_group, None);
    }

    #[test]
    fn caret_arrows_can_leave_the_first_group_and_return_to_new_group() {
        use iced::advanced::{layout, renderer::Headless, widget::Tree};
        use iced::{Font, Pixels, Size, keyboard, mouse};
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.67),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut a = app();
        for tag in ["genre:bachata", "mood:romantic"] {
            let _ = a.update(Message::ToggleTag(tag.into()));
            let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        }
        a.filter
            .retained
            .tag_blocks
            .push(vec![weave_player_lib::core::TagFilter {
                any: vec!["practice".into()],
                ..Default::default()
            }]);
        let mut tree = Tree::empty();
        for (key, expected) in [
            (keyboard::key::Named::ArrowLeft, Some((1, 0))),
            (keyboard::key::Named::ArrowLeft, Some((0, 1))),
            (keyboard::key::Named::ArrowLeft, Some((0, 0))),
            (keyboard::key::Named::ArrowRight, Some((0, 1))),
            (keyboard::key::Named::ArrowRight, Some((1, 0))),
            (keyboard::key::Named::ArrowRight, None),
            (keyboard::key::Named::ArrowLeft, Some((1, 0))),
            (keyboard::key::Named::ArrowRight, None),
        ] {
            let messages = {
                let mut element = a.filter_editor(FilterTarget::Jam);
                tree.diff(element.as_widget());
                let size = Size::new(480.0, 800.0);
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                let mut focus = iced::advanced::widget::operation::focusable::focus::<()>(
                    iced::widget::Id::new(crate::ui_dialogs::TAG_QUERY_ID),
                );
                element.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut focus,
                );
                let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(key),
                    modified_key: keyboard::Key::Named(key),
                    physical_key: keyboard::key::Physical::Unidentified(
                        keyboard::key::NativeCode::Unidentified,
                    ),
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::empty(),
                    text: None,
                    repeat: false,
                });
                let mut messages = Vec::new();
                let mut shell = iced::advanced::Shell::new(&mut messages);
                element.as_widget_mut().update(
                    &mut tree,
                    &event,
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                assert!(shell.is_event_captured());
                messages
            };
            assert!(
                messages.iter().any(|message| matches!(
                    message, Message::SelectTagGroup(FilterTarget::Jam, group) if *group == expected
                )),
                "caret must navigate to {expected:?}, got {messages:?}"
            );
            for message in messages {
                let _ = a.update(message);
            }
            assert_eq!(
                a.active_tag_group,
                expected.map(|(block, index)| (FilterTarget::Jam, block, index))
            );
        }
    }

    #[test]
    fn selected_and_grouped_tags_share_input_and_page_rendering() {
        let mut a = app();
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        assert_eq!(
            a.tag_selection_chips(
                &a.filter.tags,
                &[],
                Some(Message::ToggleTag),
                Some(FilterTarget::Jam)
            )
            .len(),
            1
        );
        assert_eq!(
            a.tag_selection_chips(&a.filter.tags, &[], None, None).len(),
            1
        );
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        let grouped = &a.filter.retained.tag_blocks;
        assert_eq!(
            a.tag_selection_chips(
                &[],
                grouped,
                Some(Message::ToggleTag),
                Some(FilterTarget::Jam)
            )
            .len(),
            1,
            "one group without a trailing comma"
        );
        assert_eq!(a.tag_selection_chips(&[], grouped, None, None).len(), 1);
        assert!(a.filter_sentence(&a.filter).contains("romantic"));
        let _ = a.update(Message::ToggleTag("genre:bachata".into()));
        assert_eq!(
            a.tag_selection_chips(&a.filter.tags, &a.filter.retained.tag_blocks, None, None)
                .len(),
            3
        );
    }

    #[test]
    fn new_tag_groups_are_ored_immediately_and_after_commit_or_reload() {
        let mut a = app();
        let mut expected = Vec::new();
        for tag in ["mood:romantic", "genre:bachata"] {
            a.filter = Filter::default();
            let _ = a.update(Message::FilterTagToggle(FilterTarget::Jam, tag.into()));
            expected.extend(a.rows.iter().map(|track| track.id));
        }
        expected.sort_unstable();
        expected.dedup();
        a.filter = Filter::default();
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Jam,
            "mood:romantic".into(),
        ));
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Jam,
            "genre:bachata".into(),
        ));
        let ids = |a: &App| {
            let mut ids: Vec<_> = a.rows.iter().map(|track| track.id).collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(
            ids(&a),
            expected,
            "new group must join the union before pressing plus again"
        );
        assert!(a.filter.to_spec().tag_all.is_empty());
        assert_eq!(a.filter.to_spec().tag_blocks[0].len(), 2);
        assert_eq!(
            a.tag_selection_chips(&a.filter.tags, &a.filter.retained.tag_blocks, None, None)
                .len(),
            3
        );
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        assert_eq!(
            ids(&a),
            expected,
            "finishing a group must not change the union"
        );
        let spec = a.filter.to_spec();
        a.filter = Filter::from_spec(&spec);
        a.refresh();
        assert_eq!(
            ids(&a),
            expected,
            "saved/reloaded group nesting must keep OR semantics"
        );
        assert_eq!(a.filter.to_spec(), spec);
    }

    #[test]
    fn shared_tag_constraints_in_imported_filters_stay_anded_with_or_groups() {
        let mut a = app();
        let spec = weave_player_lib::core::FilterSpec {
            tag_all: vec!["mood:romantic".into()],
            tag_blocks: vec![vec![weave_player_lib::core::TagFilter {
                any: vec!["genre:bachata".into()],
                ..Default::default()
            }]],
            ..Default::default()
        };
        a.filter = Filter::from_spec(&spec);
        let _ = a.update(Message::FilterTagToggle(
            FilterTarget::Jam,
            "practice".into(),
        ));
        assert_eq!(a.filter.to_spec().tag_all, ["mood:romantic", "practice"]);
        assert_eq!(a.filter.to_spec().tag_blocks, spec.tag_blocks);
    }

    #[test]
    fn plus_groups_selected_tags_with_or_logic() {
        let mut a = app();
        let mut expected = Vec::new();
        for tag in ["mood:romantic", "genre:bachata"] {
            a.filter = Filter::default();
            let _ = a.update(Message::ToggleTag(tag.into()));
            expected.extend(a.rows.iter().map(|track| track.id));
        }
        expected.sort_unstable();
        expected.dedup();
        assert!(!expected.is_empty());
        a.filter = Filter::default();
        for tag in ["mood:romantic", "genre:bachata"] {
            let _ = a.update(Message::ToggleTag(tag.into()));
        }
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        let mut actual: Vec<_> = a.rows.iter().map(|track| track.id).collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "plus must match either selected tag");
        let group = &a.filter.retained.tag_blocks[0][0];
        assert!(group.all.is_empty());
        assert_eq!(group.any, ["mood:romantic", "genre:bachata"]);
    }

    #[test]
    fn grouped_tags_are_removed_individually_and_keep_other_group_caret_positions() {
        let mut a = app();
        for tags in [vec!["genre:bachata", "genre:zouk"], vec!["mood:romantic"]] {
            for tag in tags {
                let _ = a.update(Message::FilterTagToggle(FilterTarget::Jam, tag.into()));
            }
            let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        }
        let _ = a.update(Message::SelectTagGroup(FilterTarget::Jam, Some((0, 1))));
        let _ = a.update(Message::RemoveGroupedTag(
            FilterTarget::Jam,
            0,
            0,
            "genre:bachata".into(),
        ));
        assert_eq!(a.filter.retained.tag_blocks[0][0].all, ["genre:zouk"]);
        assert_eq!(a.active_tag_group, Some((FilterTarget::Jam, 0, 1)));
        let _ = a.update(Message::RemoveGroupedTag(
            FilterTarget::Jam,
            0,
            0,
            "genre:zouk".into(),
        ));
        assert_eq!(a.filter.retained.tag_blocks[0][0].all, ["mood:romantic"]);
        assert_eq!(a.active_tag_group, Some((FilterTarget::Jam, 0, 0)));
        let _ = a.update(Message::RemoveGroupedTag(
            FilterTarget::Jam,
            0,
            0,
            "mood:romantic".into(),
        ));
        assert!(a.filter.retained.tag_blocks.is_empty());
        assert_eq!(a.active_tag_group, None);
        let _ = a.update(Message::RemoveGroupedTag(
            FilterTarget::Jam,
            0,
            0,
            "mood:romantic".into(),
        ));
        assert!(
            a.filter.retained.tag_blocks.is_empty(),
            "stale remove must not re-add a tag"
        );
    }

    #[test]
    fn selection_groups_are_removable_and_preserved_in_saved_definitions() {
        let mut a = app();
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let before = a.rows.clone();
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        assert!(a.filter.tags.is_empty());
        assert_eq!(a.filter.to_spec().tag_blocks[0][0].all, ["mood:romantic"]);
        assert_eq!(a.rows, before);
        assert!(a.filter.is_saveable());
        assert!(
            a.active_chips().is_some(),
            "Jam must display committed groups even without loose tags"
        );
        let _ = a.jam_page();
        let spec = a.filter.to_spec();
        assert_eq!(Filter::from_spec(&spec).to_spec(), spec);
        let _ = a.view();
        let _ = a.update(Message::RemoveGroupedTag(
            FilterTarget::Jam,
            0,
            0,
            "mood:romantic".into(),
        ));
        assert!(a.filter.to_spec().tag_blocks.is_empty());
        assert!(a.active_chips().is_none());
        assert!(
            a.rows.len() >= before.len(),
            "removing a group refreshes the matches"
        );
        let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
        assert!(a.filter.to_spec().tag_blocks.is_empty());
    }

    #[test]
    fn exclusions_with_commas_and_backslashes_survive_unrelated_gui_edits() {
        let spec = weave_player_lib::core::FilterSpec {
            tag_exclude: vec!["guest, live".into(), r"path\tag".into(), "mood:calm".into()],
            ..Default::default()
        };
        let mut f = Filter::from_spec(&spec);
        assert_eq!(f.to_spec(), spec);
        f.artists.push("Artist".into());
        assert_eq!(f.to_spec().tag_exclude, spec.tag_exclude);
    }
    #[test]
    fn native_folder_selection_starts_import_and_blocks_duplicate_submissions() {
        let dir = import_picker::tests::temp();
        let music = dir.join("music");
        std::fs::create_dir(&music).unwrap();
        let song = music.join("song.wav");
        import_picker::tests::wav(&song);
        let db = dir.join("library.db");
        let (backend, library) = backend::sample::disk(&db);
        let mut a = App::with(backend, library).0;
        let _ = a.update(Message::Import(import_picker::Mode::Folder));
        assert!(!a.import_in_progress);
        let _ = a.update(Message::ImportChosen(a.import_request, vec![music.clone()]));
        assert!(!a.import_chooser_open);
        assert!(a.import_in_progress);
        let request = a.import_request;
        let _ = a.update(Message::Import(import_picker::Mode::Folder));
        assert_eq!(a.import_request, request);
        assert!(!a.import_chooser_open);
        let _ = a.update(Message::Picked(vec![song.clone()]));
        assert!(a.import_in_progress);
        let report = backend::import(&db, &[music, song]).unwrap();
        assert_eq!(report.added, 1);
        let _ = a.update(Message::Imported(Ok(report)));
        assert!(!a.import_in_progress);
        assert!(a.import_report.is_none());
        assert_eq!(a.lib.tracks.len(), 1);
        drop(a);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn stale_analytics_responses_do_not_replace_current_statistics() {
        let mut a = app();
        let _ = a.update(Message::Open(Dest::AboutMe));
        let current = a.analytics_request;
        assert!(a.analytics_result.as_ref().unwrap().is_ok());
        let _ = a.update(Message::AnalyticsLoaded(
            current.saturating_sub(1),
            Box::new(Err("stale response".into())),
        ));
        assert!(a.analytics_result.as_ref().unwrap().is_ok());
    }

    #[test]
    fn explicitly_resetting_bpm_removes_saved_open_bounds_without_losing_other_rules() {
        let spec = weave_player_lib::core::FilterSpec {
            bpm_max: Some(180.0),
            rating_min: Some(4),
            ..Default::default()
        };
        let mut filter = Filter::from_spec(&spec);
        assert_eq!(filter.to_spec(), spec);
        assert_eq!(filter.bpm_label(), "up to 180 BPM");
        filter.set_bpm(data::BPM);
        let updated = filter.to_spec();
        assert_eq!(updated.bpm_max, None);
        assert_eq!(updated.rating_min, Some(4));
    }

    #[test]
    fn full_saved_filter_survives_gui_edits_and_membership_uses_engine_results() {
        use weave_player_lib::core::{FilterSpec, Sort, SortField};
        let mut a = app();
        let spec = FilterSpec {
            query: Some("certainly no match".into()),
            rating_min: Some(4),
            tag_exclude: vec!["live".into()],
            limit: Some(2),
            bpm_min: Some(90.0),
            sort: Some(Sort {
                field: SortField::Added,
                descending: true,
            }),
            ..Default::default()
        };
        backend::sample::dynamic(&mut a.backend, "Precise", spec.clone());
        assert!(a.reload_library());
        let index = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == "Precise")
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        assert!(a.rows.is_empty());
        let _ = a.update(Message::EditFilter);
        let draft = a.draft.as_mut().unwrap();
        assert_eq!(draft.to_spec(), spec);
        draft.tags.push("mood:romantic".into());
        let _ = a.update(Message::SaveFilter);
        let saved = a
            .lib
            .playlists
            .iter()
            .find(|p| p.name == "Precise")
            .unwrap()
            .filter
            .as_ref()
            .unwrap()
            .to_spec();
        assert_eq!(saved.query, spec.query);
        assert_eq!(saved.rating_min, spec.rating_min);
        assert_eq!(saved.limit, spec.limit);
        assert_eq!(saved.sort, spec.sort);
        assert_eq!(saved.bpm_max, None);
        assert_eq!(saved.tag_exclude, spec.tag_exclude);
        assert!(a.rows.is_empty());
    }
    #[test]
    fn inline_search_filters_the_current_playlist_source_and_closing_restores_it() {
        let mut a = app();
        let included = a.lib.tracks[0].clone();
        let outside = a
            .lib
            .tracks
            .iter()
            .find(|t| t.title != included.title)
            .unwrap()
            .clone();
        let _ = a.update(Message::ToggleFavoriteTrack(included.id));
        let index = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        let baseline = a.rows.clone();
        let _ = a.update(Message::SearchOpen(true));
        assert!(a.search_open);
        assert!(!a.dialog_open_for_test());
        let _ = a.update(Message::SearchText(outside.title));
        assert!(
            a.rows.is_empty(),
            "a matching library track outside the source must stay out"
        );
        assert_eq!(a.context_search_spec().sources, [backend::FAVOURITES]);
        let _ = a.view();
        let _ = a.update(Message::Resized(390.0, 844.0));
        assert!(a.search_open);
        assert!(!a.dialog_open_for_test());
        assert!(a.sheet.is_none(), "inline search creates no bottom sheet");
        let _ = a.view();
        let _ = a.update(Message::SearchOpen(false));
        assert!(!a.search_open);
        assert_eq!(a.rows, baseline);
        let _ = a.update(Message::SearchOpen(true));
        let _ = a.update(Message::SearchText(included.title));
        assert_eq!(a.rows.len(), 1);
        let _ = a.update(Message::Open(Dest::Tracks));
        assert!(!a.search_open);
        assert!(a.collection_query.is_empty());
    }
    #[test]
    fn inline_jam_search_is_additive_to_saved_filters_and_limits() {
        use weave_player_lib::core::{FilterSpec, Sort, SortField};
        let mut a = app();
        a.filter = Filter::from_spec(&FilterSpec {
            limit: Some(1),
            sort: Some(Sort {
                field: SortField::Rating,
                descending: true,
            }),
            ..Default::default()
        });
        let _ = a.update(Message::ToggleJam);
        let baseline: Vec<_> = a.rows.iter().map(|t| t.id).collect();
        assert_eq!(baseline.len(), 1);
        let outside = a
            .lib
            .tracks
            .iter()
            .find(|t| !baseline.contains(&t.id) && t.title != a.rows[0].title)
            .unwrap()
            .title
            .clone();
        let original = a.filter.to_spec();
        let _ = a.update(Message::SearchOpen(true));
        let _ = a.update(Message::SearchText(outside));
        assert!(
            a.rows.is_empty(),
            "search must not select a new track before the saved limit"
        );
        assert_eq!(a.filter.to_spec(), original);
        let _ = a.update(Message::SearchOpen(false));
        assert_eq!(a.rows.iter().map(|t| t.id).collect::<Vec<_>>(), baseline);
    }
    #[test]
    fn artist_and_album_search_do_not_escape_their_existing_track_sets() {
        let mut a = app();
        for dest in [Dest::Artist(0), Dest::Album(0)] {
            let _ = a.update(Message::Open(dest));
            let baseline: Vec<_> = a.rows.iter().map(|t| t.id).collect();
            assert!(!baseline.is_empty());
            let outside = a
                .lib
                .tracks
                .iter()
                .find(|t| !baseline.contains(&t.id))
                .unwrap()
                .clone();
            let _ = a.update(Message::SearchOpen(true));
            let _ = a.update(Message::SearchText(outside.title));
            assert!(a.rows.iter().all(|t| baseline.contains(&t.id)));
            assert!(!a.rows.iter().any(|t| t.id == outside.id));
            let _ = a.update(Message::EscapeKey);
            assert!(!a.search_open);
            assert_eq!(a.rows.iter().map(|t| t.id).collect::<Vec<_>>(), baseline);
        }
    }
    #[test]
    fn manual_playlist_creation_and_membership_reuse_the_playlist_page_without_changing_playback() {
        let mut a = app();
        backend::sample::playlist(&mut a.backend, "Existing", "");
        assert!(a.reload_library());
        a.play(vec![8, 9], 0);
        let queue = a.player.queue.clone();
        let _ = a.update(Message::Open(Dest::Playlists));
        let before = a.lib.playlists.len();
        let track_count = a.lib.tracks.len();
        let _ = a.update(Message::NewManualPlaylist);
        assert!(a.save_manual && a.save_name.is_some());
        let _ = a.update(Message::SaveNameInput(" ".into()));
        let _ = a.update(Message::ConfirmSaveJam);
        assert_eq!(a.lib.playlists.len(), before);
        assert!(a.save_name.is_some());
        let _ = a.update(Message::SaveNameInput("Existing".into()));
        let _ = a.update(Message::ConfirmSaveJam);
        assert_eq!(a.lib.playlists.len(), before);
        let _ = a.update(Message::CancelSaveJam);
        assert!(a.save_name.is_none());
        let _ = a.update(Message::NewManualPlaylist);
        let _ = a.update(Message::SaveNameInput(" My manual set ".into()));
        let _ = a.update(Message::ConfirmSaveJam);
        let playlist = a
            .manual_playlist()
            .expect("opens the created manual playlist");
        assert_eq!(playlist.name, "My manual set");
        assert!(playlist.tracks.is_empty());
        assert!(
            a.backend
                .playlist_order("My manual set")
                .unwrap()
                .is_empty()
        );
        let _ = a.update(Message::PlaylistTracksOpen(true));
        assert!(a.playlist_tracks_open);
        let _ = a.view();
        let _ = a.update(Message::PlaylistTracksQuery(a.track(1).title.clone()));
        for id in [2, 0, 1] {
            let _ = a.update(Message::PlaylistTrackToggle(id));
        }
        let saved = a.backend.playlist_order("My manual set").unwrap();
        assert_eq!(saved, [2, 0, 1].map(|id| a.track(id).slug.clone()));
        let _ = a.update(Message::Resized(390.0, 844.0));
        let _ = a.view();
        let _ = a.update(Message::PlaylistTrackToggle(0));
        assert!(a.reload_library());
        assert_eq!(
            a.backend.playlist_order("My manual set").unwrap(),
            [2, 1].map(|id| a.track(id).slug.clone())
        );
        assert_eq!(
            a.lib.tracks.len(),
            track_count,
            "removing membership keeps the library track"
        );
        let _ = a.update(Message::EscapeKey);
        assert!(!a.playlist_tracks_open);
        assert_eq!(a.player.queue, queue);
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Edit));
        let _ = a.update(Message::PlaylistTracksOpen(true));
        assert!(
            !a.playlist_tracks_open,
            "membership cannot invalidate an open reorder draft"
        );
        let _ = a.update(Message::PlaylistTrackToggle(2));
        assert_eq!(a.backend.playlist_order("My manual set").unwrap().len(), 2);
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Cancel));
        backend::sample::dynamic(&mut a.backend, "Generated membership", Default::default());
        assert!(a.reload_library());
        let dynamic = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == "Generated membership")
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(dynamic)));
        let original = a.lib.playlists[dynamic].tracks.clone();
        let _ = a.update(Message::PlaylistTracksOpen(true));
        let _ = a.update(Message::PlaylistTrackToggle(2));
        assert!(!a.playlist_tracks_open);
        assert_eq!(a.lib.playlists[dynamic].tracks, original);
        assert!(
            a.backend
                .set_playlist_track("Generated membership", &a.track(2).slug.clone(), false)
                .is_err()
        );
    }

    #[test]
    fn curated_rules_limit_additions_and_pruning_requires_a_dialog_at_every_width() {
        let mut a = app();
        let _ = a.update(Message::Open(Dest::Playlists));
        let _ = a.update(Message::NewManualPlaylist);
        assert!(!a.dialog_open_for_test(), "creation is a full-screen page");
        let _ = a.update(Message::DraftToggle("mood:romantic".into()));
        let eligible = a.backend.matching(&a.creation_filter.to_spec()).unwrap();
        assert!(eligible.len() >= 2);
        let outside = a
            .lib
            .tracks
            .iter()
            .find(|t| !eligible.contains(&t.id))
            .unwrap()
            .id;
        let _ = a.view();
        let _ = a.update(Message::SaveNameInput("Curated rules".into()));
        let _ = a.update(Message::ConfirmSaveJam);
        assert!(a.manual_playlist().unwrap().tracks.is_empty());
        assert_eq!(
            a.manual_playlist().unwrap().filter.as_ref().unwrap().tags,
            ["mood:romantic"]
        );
        assert_eq!(
            a.backend
                .playlist_add_candidates("Curated rules", "")
                .unwrap(),
            eligible
        );
        assert!(
            a.backend
                .playlist_add_candidates("Curated rules", &a.track(outside).title)
                .unwrap()
                .is_empty()
        );
        let _ = a.update(Message::PlaylistTrackToggle(outside));
        assert!(
            a.manual_playlist().unwrap().tracks.is_empty(),
            "an ineligible addition is blocked"
        );
        for &id in &eligible[..2] {
            let _ = a.update(Message::PlaylistTrackToggle(id));
        }
        let saved = a.backend.playlist_order("Curated rules").unwrap();
        let _ = a.update(Message::EditFilter);
        let _ = a.update(Message::DraftToggle("mood:romantic".into()));
        let _ = a.update(Message::SaveFilter);
        assert!(
            a.manual_filter_review.as_ref().unwrap().removed.is_empty(),
            "removing an AND tag still confirms, with zero disqualified tracks"
        );
        assert!(a.dialog_open_for_test());
        let _ = a.update(Message::ConfirmManualFilter);
        assert_eq!(a.backend.playlist_order("Curated rules").unwrap(), saved);
        let _ = a.update(Message::PlaylistTrackToggle(outside));
        let before = a.backend.playlist_order("Curated rules").unwrap();
        let track_count = a.lib.tracks.len();
        a.play(vec![8, 9], 0);
        let queue = a.player.queue.clone();
        let _ = a.update(Message::EditFilter);
        let _ = a.update(Message::DraftToggle("mood:romantic".into()));
        let _ = a.update(Message::SaveFilter);
        assert_eq!(a.manual_filter_review.as_ref().unwrap().removed.len(), 1);
        assert!(a.dialog_open_for_test());
        assert_eq!(a.backend.playlist_order("Curated rules").unwrap(), before);
        assert!(a.manual_playlist().unwrap().filter.is_none());
        let _ = a.update(Message::CancelManualFilterReview);
        assert!(a.draft.is_some());
        assert_eq!(a.backend.playlist_order("Curated rules").unwrap(), before);
        let _ = a.update(Message::Resized(390.0, 844.0));
        let _ = a.update(Message::SaveFilter);
        assert!(
            a.dialog_open_for_test(),
            "confirmation stays a dialog on narrow"
        );
        let _ = a.view();
        let _ = a.update(Message::ConfirmManualFilter);
        assert!(a.manual_filter_review.is_none() && a.draft.is_none());
        assert_eq!(a.backend.playlist_order("Curated rules").unwrap(), saved);
        assert_eq!(a.lib.tracks.len(), track_count);
        assert_eq!(a.player.queue, queue);
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Edit));
        assert!(
            a.playlist_edit.is_some(),
            "filtered manual playlists retain curation controls"
        );
        let _ = a.view();
    }

    #[test]
    fn manual_playlist_order_is_a_cancellable_draft_and_saving_preserves_playback() {
        let mut a = app();
        for id in [0, 1, 2] {
            let _ = a.update(Message::ToggleFavoriteTrack(id));
        }
        a.play(vec![8, 9], 0);
        let queue = a.player.queue.clone();
        let index = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        let original = a.backend.playlist_order(backend::FAVOURITES).unwrap();
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Edit));
        assert!(a.playlist_edit.is_some());
        assert!(!a.can_search_page());
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Reorder {
            from: 0,
            to: 2,
        }));
        assert!(a.playlist_edit.as_ref().unwrap().order.is_dirty());
        assert_eq!(
            a.backend.playlist_order(backend::FAVOURITES).unwrap(),
            original
        );
        let _ = a.view();
        let _ = a.update(Message::Resized(390.0, 844.0));
        let _ = a.view();
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Cancel));
        assert!(a.playlist_edit.is_none());
        assert_eq!(
            a.backend.playlist_order(backend::FAVOURITES).unwrap(),
            original
        );
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Edit));
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Reorder {
            from: 0,
            to: 2,
        }));
        let draft = a.playlist_edit.as_ref().unwrap().order.items().to_vec();
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Submit));
        assert!(a.playlist_edit.is_none());
        assert_eq!(
            a.backend.playlist_order(backend::FAVOURITES).unwrap(),
            draft
        );
        assert_eq!(
            a.rows.iter().map(|t| t.slug.clone()).collect::<Vec<_>>(),
            draft
        );
        assert_eq!(a.player.queue, queue);
        backend::sample::dynamic(&mut a.backend, "Generated", Default::default());
        assert!(a.reload_library());
        let dynamic = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == "Generated")
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(dynamic)));
        let _ = a.update(Message::PlaylistOrder(ReorderEvent::Edit));
        assert!(
            a.playlist_edit.is_none(),
            "saved-filter playlists cannot be manually reordered"
        );
    }
    #[test]
    fn sort_selection_cycles_descending_ascending_and_deselected() {
        use weave_player_lib::core::CollectionOrder;
        let mut a = app();
        let _ = a.update(Message::Open(Dest::Tracks));
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert!(a.collection_descending());
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert!(!a.collection_descending());
        let rated: Vec<_> = a.rows.iter().filter(|t| t.rating > 0).collect();
        assert!(
            rated
                .windows(2)
                .all(|pair| pair[0].rating <= pair[1].rating)
        );
        let _ = a.update(Message::CollectionQuery(a.rows[0].artist.clone()));
        assert!(!a.collection_descending(), "search retains direction");
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert_eq!(a.selected_collection_order(), None);
        assert_eq!(
            a.rows.iter().map(|t| t.id).collect::<Vec<_>>(),
            a.backend
                .collection_tracks(&a.collection_query, None, true)
                .unwrap()
        );
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert!(a.collection_descending());
        let _ = a.update(Message::ToggleJam);
        for _ in 0..3 {
            let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        }
        assert_eq!(a.filter.to_spec().sort, None);
        let _ = a.update(Message::Open(Dest::Album(0)));
        let baseline: Vec<_> = a.rows.iter().map(|t| t.id).collect();
        for _ in 0..3 {
            let _ = a.update(Message::CollectionOrder(CollectionOrder::AddedDate));
        }
        assert_eq!(a.detail_order, None);
        assert_eq!(a.rows.iter().map(|t| t.id).collect::<Vec<_>>(), baseline);
    }
    #[test]
    fn collection_sort_survives_search_and_is_kept_separately_per_library_page() {
        use weave_player_lib::core::CollectionOrder;
        let mut a = app();
        let _ = a.update(Message::Open(Dest::Tracks));
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert!(
            a.rows
                .windows(2)
                .all(|pair| pair[0].rating >= pair[1].rating)
        );
        let title = a.rows[0].title.clone();
        let _ = a.update(Message::CollectionQuery(title));
        assert!(!a.rows.is_empty());
        assert_eq!(a.collection_order(), CollectionOrder::Rating);
        let _ = a.update(Message::Open(Dest::Albums));
        let _ = a.update(Message::CollectionOrder(CollectionOrder::ReleaseDate));
        let _ = a.view();
        let _ = a.update(Message::Resized(390.0, 844.0));
        let _ = a.view();
        let _ = a.update(Message::Open(Dest::Tracks));
        assert_eq!(a.collection_order(), CollectionOrder::Rating);
        assert!(a.collection_query.is_empty());
        assert!(
            a.rows
                .windows(2)
                .all(|pair| pair[0].rating >= pair[1].rating)
        );
    }
    #[test]
    fn jam_sort_is_ephemeral_and_detail_sort_preserves_playlist_membership_and_saved_order() {
        use weave_player_lib::core::CollectionOrder;
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::JamRecent(true));
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert_eq!(a.filter.to_spec().sort, None);
        assert_eq!(a.jam_order, Some(CollectionOrder::Rating));
        save_jam(&mut a);
        let saved_name = a
            .lib
            .playlists
            .iter()
            .find(|p| p.filter.is_some())
            .unwrap()
            .name
            .clone();
        assert!(a.reload_library());
        assert_eq!(
            a.lib
                .playlists
                .iter()
                .find(|p| p.name == saved_name)
                .unwrap()
                .filter
                .as_ref()
                .unwrap()
                .to_spec()
                .sort,
            None
        );
        assert!(
            a.rows
                .windows(2)
                .all(|pair| pair[0].rating >= pair[1].rating)
        );
        let _ = a.view();
        for id in [4, 1, 3] {
            let _ = a.update(Message::ToggleFavoriteTrack(id));
        }
        let index = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .unwrap();
        let saved_ids: Vec<_> = a
            .lib
            .playlist_tracks(&a.lib.playlists[index])
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(saved_ids.len(), 3);
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        let mut ids: Vec<_> = a.rows.iter().map(|t| t.id).collect();
        let mut expected = saved_ids.clone();
        ids.sort();
        expected.sort();
        assert_eq!(ids, expected);
        assert!(
            a.rows
                .windows(2)
                .all(|pair| pair[0].rating >= pair[1].rating)
        );
        assert_eq!(
            a.lib
                .playlist_tracks(&a.lib.playlists[index])
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            saved_ids
        );
        let _ = a.update(Message::Open(Dest::Home));
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        assert_eq!(a.rows.iter().map(|t| t.id).collect::<Vec<_>>(), saved_ids);
    }
    #[test]
    fn temporary_sort_controls_playback_without_changing_saved_rules_or_shuffle_preference() {
        use weave_player_lib::core::{CollectionOrder, FilterSpec, Sort, SortField};
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        // Existing saved/CLI rules, including a sorted limit, still decide membership.
        let base = FilterSpec {
            sort: Some(Sort {
                field: SortField::Added,
                descending: true,
            }),
            limit: Some(5),
            ..Default::default()
        };
        a.filter = Filter::from_spec(&base);
        a.refresh();
        let mut members = a.rows.iter().map(|t| t.id).collect::<Vec<_>>();
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        assert_eq!(a.filter.to_spec(), base);
        let ordered = a.rows.iter().map(|t| t.id).collect::<Vec<_>>();
        let mut sorted = ordered.clone();
        sorted.sort();
        members.sort();
        assert_eq!(
            members, sorted,
            "temporary ordering cannot change limit membership"
        );
        let _ = a.update(Message::StartJam);
        assert_eq!(a.player.queue, ordered);
        assert!(!a.player.shuffle);
        assert!(!a.backend.snapshot().shuffle);
        for id in [4, 1, 3] {
            let _ = a.update(Message::ToggleFavoriteTrack(id));
        }
        let index = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(index)));
        let _ = a.update(Message::SetShufflePref(Some(true)));
        let original = a.backend.playlist_order(backend::FAVOURITES).unwrap();
        let _ = a.update(Message::CollectionOrder(CollectionOrder::Rating));
        let ordered = a.rows.iter().map(|t| t.id).collect::<Vec<_>>();
        let _ = a.update(Message::PlayAll(ordered.clone()));
        assert_eq!(a.player.queue, ordered);
        assert!(!a.player.shuffle);
        assert_eq!(
            a.lib.playlists[index].shuffle,
            Some(true),
            "sort leaves the saved preference alone"
        );
        assert_eq!(
            a.backend.playlist_order(backend::FAVOURITES).unwrap(),
            original
        );
        let _ = a.update(Message::SetShufflePref(Some(true)));
        assert!(!a.has_playback_sort());
        let ids = a.rows.iter().map(|t| t.id).collect::<Vec<_>>();
        let _ = a.update(Message::PlayAll(ids));
        assert!(a.player.shuffle);
    }

    #[test]
    fn library_search_sections_rediscover_and_about_me_use_real_engine_state() {
        let mut a = app();
        let _ = a.update(Message::Go(Dest::Tracks));
        let _ = a.update(Message::CollectionQuery("certainly no match".into()));
        assert!(a.rows.is_empty());
        for dest in [
            Dest::Artists,
            Dest::Albums,
            Dest::Playlists,
            Dest::Rediscover,
            Dest::Explore,
            Dest::AboutMe,
        ] {
            let _ = a.update(Message::Open(dest));
            let _ = a.view();
        }
        assert!(
            a.analytics_result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .listened_ms
                == 0,
            "legacy starts are not measured time"
        );
        let _ = a.update(Message::PageFilters(true));
        let _ = a.update(Message::PageExclusions("live, mood:sad".into()));
        assert_eq!(
            a.analytics_filter.to_spec().tag_exclude,
            ["live", "mood:sad"]
        );
        let _ = a.update(Message::PageFilters(false));
        let _ = a.update(Message::AnalyticsPeriod(None));
        let range = iad::primitives::range::DateRange {
            start: iad::primitives::date::Date::new(2026, 11, 1),
            end: iad::primitives::date::Date::new(2026, 11, 2),
        };
        let _ = a.update(Message::AnalyticsDatePick(range));
        let q = a.analytics_query().unwrap();
        assert_eq!(q.until_ms.unwrap() - q.since_ms.unwrap(), 2 * 86_400_000);
        let _ = a.update(Message::Go(Dest::Explore));
        let _ = a.update(Message::ExploreSection(0));
        assert!(!a.explore_open[0]);
        let _ = a.view();
        let _ = a.update(Message::Go(Dest::Rediscover));
        let _ = a.update(Message::RediscoverDays("90".into()));
        assert_eq!(a.while_days, 90);
        let _ = a.update(Message::RediscoverSort(
            weave_player_lib::core::SortField::Rating,
        ));
        let _ = a.view();
    }
    #[test]
    fn native_import_chooser_cancels_without_a_page_and_rejects_stale_replies() {
        let mut a = app();
        let before = a.lib.tracks.len();
        let _ = a.update(Message::Import(import_picker::Mode::Files));
        let request = a.import_request;
        assert!(a.import_chooser_open);
        assert!(a.import_report.is_none());
        assert!(!a.dialog_open_for_test());
        let _ = a.update(Message::Import(import_picker::Mode::Folder));
        assert_eq!(a.import_request, request, "do not open a second chooser");
        let _ = a.update(Message::ImportChosen(request, Vec::new()));
        assert!(!a.import_chooser_open);
        assert!(!a.import_in_progress);
        assert_eq!(a.lib.tracks.len(), before);
        let _ = a.update(Message::Import(import_picker::Mode::Folder));
        assert!(a.import_request > request);
        let _ = a.update(Message::ImportChosen(request, vec!["stale.wav".into()]));
        assert!(a.import_chooser_open);
        assert!(!a.import_in_progress);
        let _ = a.update(Message::ImportChosen(a.import_request, Vec::new()));
        assert!(!a.import_chooser_open);
    }
    #[test]
    fn partial_import_reloads_successes_and_keeps_the_issue_report() {
        let dir = import_picker::tests::temp();
        let music = dir.join("music");
        std::fs::create_dir(&music).unwrap();
        import_picker::tests::wav(&music.join("song.wav"));
        let db = dir.join("library.db");
        let (backend, library) = backend::sample::disk(&db);
        let mut a = App::with(backend, library).0;
        let paths = vec![music, dir.join("missing")];
        let _ = a.update(Message::Import(import_picker::Mode::Files));
        let _ = a.update(Message::ImportChosen(a.import_request, paths.clone()));
        let report = backend::import(&db, &paths).unwrap();
        let _ = a.update(Message::Imported(Ok(report)));
        assert_eq!(a.lib.tracks.len(), 1);
        assert_eq!(a.import_report.as_ref().unwrap().issues.len(), 1);
        assert!(!a.import_in_progress);
        let _ = a.view();
        let _ = a.update(Message::DismissImportReport);
        drop(a);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn individual_tags_inherit_their_category_color() {
        let mut a = app();
        a.lib.styles.push((
            "occasion:wedding".into(),
            data::TagStyle {
                icon: "star".into(),
                color: iced::Color::BLACK,
            },
        ));
        for (tag, category) in [
            ("genre:bachata", Some("genre")),
            ("genre:zouk", Some("genre")),
            ("lang:tamil", Some("lang")),
            ("occasion:wedding", Some("occasion")),
            ("occasion:party", Some("occasion")),
            ("practice", None),
            ("90s", None),
        ] {
            assert_eq!(
                a.lib.style_of(tag).color,
                data::category_style(category).color,
                "{tag}"
            );
        }
        assert_eq!(a.lib.style_of("occasion:wedding").icon, "star");
    }

    #[test]
    fn filter_picker_defaults_to_categories_and_tag_icon_opens_all_tags() {
        let mut a = app();
        let _ = a.update(Message::JamFilters(true));
        assert_eq!(a.tag_browse, TagBrowse::Categories);
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let filter = a.filter.clone();
        let _ = a.update(Message::TagBrowseToggle);
        assert_eq!(a.tag_browse, TagBrowse::All);
        let _ = a.update(Message::TagBrowseToggle);
        assert_eq!(a.tag_browse, TagBrowse::Categories);
        assert_eq!(a.filter, filter);
        assert_ne!(
            data::category_style(Some("genre")).color,
            data::category_style(Some("lang")).color
        );
        assert_ne!(
            data::category_style(None).color,
            data::category_style(Some("lang")).color
        );
        assert_eq!(
            data::category_style(Some("occasion")).color,
            data::category_style(Some("occasion")).color
        );
    }

    #[test]
    fn tag_category_navigation_keeps_live_filters_drafts_and_track_selections() {
        let mut a = app();
        let _ = a.update(Message::JamFilters(true));
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let selected = a.filter.clone();
        let _ = a.update(Message::TagBrowseToggle);
        assert_eq!(a.tag_browse, TagBrowse::All);
        let _ = a.update(Message::TagQuery("sensual".into()));
        assert_eq!(
            a.backend.tag_categories(&a.tag_query).unwrap(),
            [Some("genre".into())]
        );
        let _ = a.update(Message::TagCategory(Some("genre".into())));
        assert_eq!(a.tag_browse, TagBrowse::Category(Some("genre".into())));
        let choices = a.backend.tag_choices(&a.tag_query, Some("genre")).unwrap();
        assert!(choices.iter().any(|t| t.name == "genre:bachata-sensual"));
        assert!(choices.iter().all(|t| t.key.as_deref() == Some("genre")));
        let _ = a.update(Message::TagCategories);
        assert_eq!(a.tag_query, "sensual");
        assert_eq!(a.filter, selected);
        let _ = a.update(Message::TagBrowseToggle);
        assert_eq!(a.tag_browse, TagBrowse::All);
        assert_eq!(a.filter, selected);
        let _ = a.update(Message::TagQuery(String::new()));
        let _ = a.update(Message::TagCategory(None));
        let custom = a.backend.tag_choices("", Some("")).unwrap();
        assert!(!custom.is_empty());
        assert!(custom.iter().all(|t| t.key.is_none()));
        assert!(custom.iter().any(|t| t.name == "practice"));
        let _ = a.update(Message::TagCategories);
        let _ = a.update(Message::TagQuery("Custom".into()));
        let _ = a.update(Message::TagCategory(None));
        assert!(
            a.tag_query.is_empty(),
            "a category-only query does not hide its children"
        );
        assert_eq!(a.filter, selected);
        let _ = a.view();

        a.draft = Some(Filter::default());
        let _ = a.update(Message::DraftToggle("practice".into()));
        let draft = a.draft.clone();
        let _ = a.update(Message::TagCategories);
        let _ = a.update(Message::TagCategory(Some("mood".into())));
        let _ = a.view();
        assert_eq!(a.draft, draft);
        assert_eq!(a.filter, selected);

        a.draft = None;
        let _ = a.update(Message::JamFilters(false));
        let original = a.lib.tracks[0].tags.clone();
        let _ = a.update(Message::EditTrack(0));
        assert_eq!(a.tag_browse, TagBrowse::All);
        let _ = a.update(Message::EditToggleTag("practice".into()));
        let track_selection = a.edit_track.clone();
        let _ = a.update(Message::TagCategory(None));
        let _ = a.update(Message::TagCategories);
        let _ = a.view();
        assert_eq!(
            a.edit_track.as_ref().unwrap().tags,
            track_selection.unwrap().tags
        );
        assert_eq!(
            a.lib.tracks[0].tags, original,
            "navigation never writes annotations"
        );
        let _ = a.update(Message::TagQuery("no such tag".into()));
        assert!(a.backend.tag_categories(&a.tag_query).unwrap().is_empty());
        let _ = a.view();
    }

    #[test]
    fn chip_expansion_is_independent_and_does_not_change_filter_selection() {
        let mut a = app();
        assert_eq!(a.chip_line_limit, 3);
        assert_eq!(a.name_chip_line_limit, 2);
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let selected = a.filter.clone();
        let _ = a.update(Message::JamFilters(true));
        for picker in [ChipPicker::Tags, ChipPicker::Artists, ChipPicker::Albums] {
            let _ = a.update(Message::ChipExpand(picker, true));
            assert!(a.chip_expanded[picker as usize]);
        }
        let _ = a.update(Message::ChipExpand(ChipPicker::Artists, false));
        assert_eq!(a.chip_expanded, [true, false, true, false]);
        let _ = a.update(Message::TagQuery("calm".into()));
        assert_eq!(a.filter, selected);
        let _ = a.view();
        let _ = a.update(Message::JamFilters(false));
        let _ = a.update(Message::JamFilters(true));
        assert_eq!(a.chip_expanded, [false; 4]);
        assert_eq!(a.filter, selected);
        let _ = a.update(Message::EditTrack(0));
        let _ = a.update(Message::ChipExpand(ChipPicker::TrackTags, true));
        let _ = a.view();
        let _ = a.update(Message::EditTrack(1));
        assert!(!a.chip_expanded[ChipPicker::TrackTags as usize]);
    }

    #[test]
    fn render_chip_filters_in_collapsed_expanded_and_search_states() {
        use iced::advanced::{
            layout,
            renderer::{Headless, Style},
            widget::Tree,
        };
        use iced::{Font, Pixels, Point, Rectangle, Size, mouse};
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.67),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut a = app();
        let track_edit = TrackEdit {
            id: 0,
            tags: a.lib.tracks[0].tags.clone(),
        };
        for (name, width, height, expanded, query) in [
            ("wide-collapsed", 560, 520, false, ""),
            ("wide-grouped", 560, 520, false, ""),
            ("wide-selected", 560, 520, false, ""),
            ("wide-category", 560, 520, false, ""),
            ("wide-expanded", 560, 520, true, ""),
            ("narrow-collapsed", 390, 844, false, ""),
            ("narrow-expanded", 390, 844, true, ""),
            ("narrow-search", 390, 844, false, "romantic"),
            ("track-tags-collapsed", 390, 520, false, ""),
            ("track-tags-expanded", 390, 520, true, ""),
        ] {
            a.filter = Filter::default();
            a.tag_browse = TagBrowse::All;
            if name == "wide-selected" {
                a.filter.tags = vec!["genre:zouk".into(), "mood:romantic".into()];
            }
            if name == "wide-grouped" {
                a.filter.tags = vec!["genre:zouk".into(), "mood:romantic".into()];
                let _ = a.update(Message::CommitTagGroup(FilterTarget::Jam));
            }
            if name == "wide-category" {
                a.tag_browse = TagBrowse::Category(None);
            }
            a.chip_expanded = [expanded; 4];
            a.tag_query = query.into();
            let theme = a.theme();
            let mut el = if name.starts_with("track-tags") {
                a.track_dialog(&track_edit)
            } else if width < 500 {
                crate::components::responsive_dialog::panel(
                    a.filter_editor(FilterTarget::Jam),
                    true,
                    Size::new(width as f32, height as f32),
                    0.0,
                )
            } else {
                crate::components::responsive_dialog::panel(
                    a.filter_editor(FilterTarget::Jam),
                    false,
                    Size::new(width as f32, height as f32),
                    0.0,
                )
            };
            let mut tree = Tree::new(&el);
            let size = Size::new(width as f32, height as f32);
            let node = el.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            if name.starts_with("track-tags") {
                assert_eq!(node.size().width, size.width);
                assert!(node.size().height <= size.height);
            } else {
                assert!(node.size().width <= size.width && node.size().height <= size.height);
            }
            let physical = Size::new(width, node.size().height.ceil() as u32);
            iced::advanced::Renderer::reset(&mut renderer, Rectangle::new(Point::ORIGIN, size));
            el.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &Style {
                    text_color: theme.semantic.window.fg,
                },
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &Rectangle::new(Point::ORIGIN, size),
            );
            let pixels = renderer.screenshot(physical, 1.0, theme.semantic.window.bg);
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] != p[1] || p[1] != p[2]),
                "rendered chips have color"
            );
            if let Some(dir) = std::env::var_os("WEAVE_CHIP_REVIEW_DIR") {
                let path = std::path::PathBuf::from(dir).join(format!("{name}.png"));
                iad::dev::save_png(&iced::window::Screenshot::new(pixels, physical, 1.0), &path)
                    .unwrap();
            }
        }
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
    fn top_level_navigation_starts_over() {
        let mut a = app();
        let _ = a.update(Message::Open(Dest::Playlist(0)));
        let _ = a.update(Message::Back);
        assert_eq!(a.dest, Dest::Home);
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
        let _ = a.update(Message::Imported(Ok(Default::default())));
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
    fn a_playlist_is_deleted_after_a_question_and_favourites_never_is() {
        let (mut backend, _) = backend::sample::backend();
        backend::sample::playlist(&mut backend, "Road trip", "");
        backend::sample::playlist(&mut backend, "Party", "");
        let lib = backend.load().unwrap();
        let mut a = App::with(backend, lib).0;
        let before = a.lib.playlists.len();
        let i = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == "Road trip")
            .unwrap();
        // Not on a playlist: nothing to delete.
        let _ = a.update(Message::AskDeletePlaylist);
        assert!(a.delete_playlist.is_none());
        let _ = a.update(Message::Open(Dest::Playlist(i)));
        assert!(a.can_delete_playlist());
        // Asking opens the question (a dialog on wide); cancelling keeps everything.
        let _ = a.update(Message::AskDeletePlaylist);
        assert_eq!(a.delete_playlist.as_deref(), Some("Road trip"));
        assert!(a.dialog_open_for_test());
        let _ = a.view();
        let _ = a.update(Message::CancelDeletePlaylist);
        assert!(a.delete_playlist.is_none() && a.lib.playlists.len() == before);
        // Confirming deletes it from the library, and the app leaves its page.
        let _ = a.update(Message::AskDeletePlaylist);
        let _ = a.update(Message::ConfirmDeletePlaylist);
        assert_eq!(a.lib.playlists.len(), before - 1);
        assert!(a.lib.playlists.iter().all(|p| p.name != "Road trip"));
        assert_eq!(a.dest, Dest::Playlists);
        assert!(a.notice.as_ref().is_some_and(|n| n.0.contains("Deleted")));
        // It stays deleted when the library is read again; its tracks are all still there.
        assert!(a.reload_library());
        assert!(a.lib.playlists.iter().all(|p| p.name != "Road trip"));
        // The default Favourites playlist has no delete option and refuses.
        let _ = a.update(Message::ToggleFavoriteTrack(3));
        let fav = a
            .lib
            .playlists
            .iter()
            .position(|p| p.name == backend::FAVOURITES)
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(fav)));
        assert!(!a.can_delete_playlist());
        let _ = a.update(Message::AskDeletePlaylist);
        assert!(a.delete_playlist.is_none());
        a.delete_playlist = Some(backend::FAVOURITES.into());
        let _ = a.update(Message::ConfirmDeletePlaylist);
        assert!(
            a.lib
                .playlists
                .iter()
                .any(|p| p.name == backend::FAVOURITES)
        );
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
        let _ = a.update(Message::Imported(Ok(Default::default())));
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
        let _ = a.update(Message::Imported(Ok(Default::default())));
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
        assert_eq!(a.dest, Dest::Home);
    }

    #[test]
    fn the_jam_page_holds_the_search_and_its_table_follows_the_filters() {
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        assert_eq!(a.rows.len(), a.lib.tracks.len(), "no filters: every track");
        assert!(!a.jam_search_open, "the search bar starts closed");
        let _ = a.update(Message::JamSearchOpen(true));
        assert!(a.jam_search_open);
        // Artists: any of the chosen ones; albums likewise; then ANDed.
        let artist = a.lib.tracks[3].artist.clone();
        let _ = a.update(Message::JamArtistToggle(artist.clone()));
        let by_artist = a.rows.len();
        assert!(by_artist > 0 && by_artist < a.lib.tracks.len());
        assert!(a.rows.iter().all(|t| t.artist == artist));
        let other = a
            .lib
            .tracks
            .iter()
            .find(|t| t.artist != artist)
            .unwrap()
            .artist
            .clone();
        let _ = a.update(Message::JamArtistToggle(other.clone()));
        assert!(a.rows.len() > by_artist, "ORed: more with a second artist");
        assert!(
            a.rows
                .iter()
                .all(|t| t.artist == artist || t.artist == other)
        );
        let _ = a.update(Message::JamArtistToggle(other));
        // The search narrows further, and the bar's close drops its text.
        let title = a.rows[0].title.clone();
        let _ = a.update(Message::JamQuery(title[..3].to_string()));
        assert!(a.rows.len() <= by_artist);
        assert_eq!(a.filter.search_count(), 1);
        let _ = a.update(Message::JamSearchOpen(false));
        assert!(a.filter.q.is_empty(), "closing the bar clears the search");
        assert_eq!(a.rows.len(), by_artist);
        let _ = a.update(Message::JamAlbumToggle("zzz-no-album".into()));
        assert!(a.rows.is_empty());
        let _ = a.update(Message::JamAlbumToggle("zzz-no-album".into()));
        let _ = a.update(Message::JamArtistToggle(artist));
        assert_eq!(a.rows.len(), a.lib.tracks.len());
        // The page builds with and without the bar, and a sort applies.
        let _ = a.view();
        let _ = a.update(Message::JamSearchOpen(true));
        let _ = a.view();
        let _ = a.update(Message::Sort(Col::Title));
        let _ = a.view();
        assert_eq!(a.page_title(), "Jam");
    }

    #[test]
    fn the_search_does_not_outlive_the_jam() {
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::JamSearchOpen(true));
        let _ = a.update(Message::JamQuery("a".into()));
        assert!(!a.rows.is_empty() && a.filter.q == "a");
        let _ = a.update(Message::Go(Dest::Tracks));
        assert!(
            a.filter.q.is_empty() && !a.jam_search_open,
            "leaving the jam drops it"
        );
        let _ = a.update(Message::ToggleJam);
        assert!(!a.jam_search_open && a.filter.q.is_empty());
    }

    #[test]
    fn the_pickers_offer_the_popular_names_first_and_the_unknown_sentinel() {
        let a = app();
        let (artists, no_artist) = a.lib.artist_choices();
        assert!(!artists.is_empty());
        assert!(
            artists.windows(2).all(|w| w[0].1 >= w[1].1),
            "most tracks first"
        );
        assert_eq!(
            artists.iter().map(|(_, n)| n).sum::<usize>() + no_artist,
            a.lib.tracks.len(),
            "every track is counted once"
        );
        let (albums, no_album) = a.lib.album_choices();
        assert_eq!(
            albums.iter().map(|(_, n)| n).sum::<usize>() + no_album,
            a.lib.tracks.len()
        );
        // The Tag filters, with several artists and albums chosen, build.
        let mut a = a;
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::JamArtistToggle(artists[0].0.clone()));
        let _ = a.update(Message::JamArtistToggle(data::UNKNOWN.into()));
        let _ = a.update(Message::JamAlbumToggle(data::UNKNOWN.into()));
        let _ = a.update(Message::ArtistQuery("av".into()));
        let _ = a.update(Message::JamFilters(true));
        let _ = a.view();
        assert!(a.dialog_open_for_test());
        assert_eq!(a.filter.tag_count(), 3);
    }

    #[test]
    fn save_as_playlist_asks_for_a_name_and_checks_it() {
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        let before = a.lib.playlists.len();
        // Nothing to save: refused with a notice, no dialog.
        let _ = a.update(Message::SaveJamAsPlaylist);
        assert!(a.save_name.is_none() && !a.dialog_open_for_test());
        assert!(a.notice.as_ref().is_some_and(|n| n.0.contains("Add a")));
        // Only a search: still nothing to save.
        let _ = a.update(Message::JamQuery("a".into()));
        let _ = a.update(Message::SaveJamAsPlaylist);
        assert!(a.save_name.is_none(), "the search is never saved");
        // A tag: the dialog opens with a suggested name.
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let _ = a.update(Message::SaveJamAsPlaylist);
        let suggested = a.save_name.clone().expect("the dialog asks for a name");
        assert!(suggested.contains("romantic"), "{suggested}");
        assert!(a.dialog_open_for_test(), "wide: a dialog");
        let _ = a.view();
        // An empty name and a taken one are refused and the dialog stays.
        let _ = a.update(Message::SaveNameInput("  ".into()));
        let _ = a.update(Message::ConfirmSaveJam);
        assert!(a.save_name.is_some() && a.lib.playlists.len() == before);
        // Cancel closes it without saving.
        let _ = a.update(Message::CancelSaveJam);
        assert!(a.save_name.is_none() && a.lib.playlists.len() == before);
        // A name of the user's own saves it, and says the search text was left out.
        let _ = a.update(Message::SaveJamAsPlaylist);
        let _ = a.update(Message::SaveNameInput("My romantic set".into()));
        let _ = a.update(Message::ConfirmSaveJam);
        assert!(a.save_name.is_none());
        assert!(a.lib.playlists.iter().any(|p| p.name == "My romantic set"));
        let notice = a.notice.as_ref().map(|n| n.0.clone()).unwrap_or_default();
        assert!(
            notice.contains("My romantic set") && notice.contains("not saved"),
            "{notice}"
        );
    }

    #[test]
    fn save_as_playlist_is_a_sheet_on_narrow() {
        let mut a = app();
        let _ = a.update(Message::Resized(420.0, 760.0));
        let _ = a.update(Message::Tick(Instant::now() + Duration::from_secs(2)));
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        let _ = a.update(Message::SaveJamAsPlaylist);
        assert!(a.save_name.is_some());
        if a.is_narrow() {
            assert!(
                !a.dialog_open_for_test(),
                "narrow: a bottom sheet, not a dialog"
            );
        }
        let _ = a.view();
    }

    /// Save as playlist as the user does it: open the dialog, accept the suggested name.
    fn save_jam(a: &mut App) {
        let _ = a.update(Message::SaveJamAsPlaylist);
        let _ = a.update(Message::ConfirmSaveJam);
    }

    #[test]
    fn a_jam_can_be_saved_as_a_dynamic_playlist() {
        let mut a = app();
        let before = a.lib.playlists.len();
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        save_jam(&mut a);
        assert_eq!(a.lib.playlists.len(), before + 1);
        let p = a
            .lib
            .playlists
            .iter()
            .find(|p| p.name.contains("romantic"))
            .expect("saved under a name made of its filters");
        assert_eq!(
            p.filter.as_ref(),
            Some(&a.filter),
            "stores the filters, which the engine evaluates"
        );
        let first = p.name.clone();
        // It is in the library, not only on screen: reading it again finds it.
        assert!(a.reload_library());
        assert!(
            a.lib
                .playlists
                .iter()
                .any(|p| p.name == first && p.filter.is_some())
        );
        // Saving the same filters again makes a second playlist with its own name.
        a.jam_saved = None;
        save_jam(&mut a);
        assert_eq!(a.lib.playlists.len(), before + 2);
        assert_eq!(
            a.lib
                .playlists
                .iter()
                .filter(|p| p.name.contains("romantic"))
                .count(),
            2
        );
    }

    #[test]
    fn a_saved_jam_keeps_everything_but_the_generic_search_and_says_so() {
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        let artist = a.lib.tracks[3].artist.clone();
        a.filter.artists = vec![artist.clone()];
        a.filter.q = "a".into();
        a.refresh();
        assert!(!a.matching(&a.filter).is_empty());
        save_jam(&mut a);
        let notice = a.notice.as_ref().map(|n| n.0.clone()).unwrap_or_default();
        assert!(
            notice.contains("not saved"),
            "tells the text search is left out: {notice}"
        );
        // Read back from the library: the artist is there, the generic search is not.
        assert!(a.reload_library());
        let p = a
            .lib
            .playlists
            .iter()
            .find(|p| {
                p.filter
                    .as_ref()
                    .is_some_and(|f| f.artists == vec![artist.clone()])
            })
            .expect("the artist was saved");
        assert_eq!(p.filter.as_ref().unwrap().q, "");
        // Without a generic search the notice is a plain "Saved as".
        a.filter.q.clear();
        a.jam_saved = None;
        save_jam(&mut a);
        let notice = a.notice.as_ref().map(|n| n.0.clone()).unwrap_or_default();
        assert!(
            notice.starts_with("Saved as ") && !notice.contains("not saved"),
            "{notice}"
        );
    }

    #[test]
    fn editing_a_dynamic_playlists_filter_is_kept_and_can_remove_conditions() {
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        let artist = a.lib.tracks[3].artist.clone();
        a.filter.artists = vec![artist.clone()];
        let _ = a.update(Message::ToggleTag("mood:romantic".into()));
        save_jam(&mut a);
        let i = a
            .lib
            .playlists
            .iter()
            .position(|p| {
                p.filter
                    .as_ref()
                    .is_some_and(|f| f.artists == vec![artist.clone()])
            })
            .unwrap();
        let _ = a.update(Message::Open(Dest::Playlist(i)));
        let _ = a.update(Message::EditFilter);
        // Remove the artist and the tag in the draft, and save.
        if let Some(d) = a.draft.as_mut() {
            d.artists.clear();
            d.tags.clear();
            d.bpm = (90.0, 120.0);
        }
        let _ = a.update(Message::SaveFilter);
        assert!(a.reload_library());
        let p = &a.lib.playlists[a
            .lib
            .playlists
            .iter()
            .position(|p| p.filter.as_ref().is_some_and(|f| f.bpm == (90.0, 120.0)))
            .expect("the edit was stored")];
        let f = p.filter.as_ref().unwrap();
        assert!(
            f.artists.is_empty() && f.tags.is_empty(),
            "conditions were removed"
        );
    }

    #[test]
    fn a_filter_matches_tags_tempo_any_of_the_artists_any_of_the_albums_and_the_search() {
        let a = app();
        let matches =
            |filter: &Filter, track: &Track| a.matching(filter).iter().any(|t| t.id == track.id);
        let t = &a.lib.tracks[3];
        let other = a.lib.tracks.iter().find(|x| x.artist != t.artist).unwrap();
        let mut f = Filter::default();
        assert!(matches(&f, t) && f.is_empty() && f.count() == 0);
        // Artists: any of them, exact and case-insensitive.
        f.artists = vec![t.artist.to_uppercase()];
        assert!(matches(&f, t) && !matches(&f, other));
        f.artists = vec!["zzz-nobody".into(), other.artist.clone()];
        assert!(!matches(&f, t) && matches(&f, other), "ORed within");
        f.artists = vec![t.artist[..2].to_string()];
        assert!(!matches(&f, t), "exact, not a substring");
        // Both artists chosen: the two tracks' artists are both accepted.
        f.artists = vec![t.artist.clone(), other.artist.clone()];
        assert!(matches(&f, t) && matches(&f, other));
        // ANDed across: an artist and an album that do not go together match nothing.
        f.artists = vec![t.artist.clone()];
        f.albums = vec!["zzz-no-album".into()];
        assert!(!matches(&f, t), "artists AND albums");
        f.albums.clear();
        // The generic search reaches the title.
        f.artists.clear();
        f.q = t.title[..2].to_string();
        assert!(matches(&f, t));
        f.q = "zzz-nothing".into();
        assert!(!matches(&f, t));
        // Counting: tags, tempo, artists and albums are the tag filters; q the search.
        f.q = "x".into();
        f.artists = vec!["y".into(), "z".into()];
        f.albums = vec!["w".into()];
        f.bpm = (90.0, 120.0);
        assert_eq!((f.count(), f.tag_count(), f.search_count()), (5, 4, 1));
        assert!(f.is_saveable());
        f.artists.clear();
        f.albums.clear();
        f.bpm = data::BPM;
        assert!(!f.is_saveable(), "the search alone is not a playlist");
        // What is stored: everything but the generic search, and it reads back the same.
        f.artists = vec!["y".into()];
        f.albums = vec!["w".into()];
        f.bpm = (90.0, 120.0);
        let spec = f.to_spec();
        assert_eq!(spec.artist_any, ["y"]);
        assert_eq!(spec.album_any, ["w"]);
        assert_eq!((spec.bpm_min, spec.bpm_max), (Some(90.0), Some(120.0)));
        let back = Filter::from_spec(&spec);
        assert_eq!(back.q, "");
        assert_eq!(
            (back.artists, back.albums, back.bpm),
            (f.artists, f.albums, (90.0, 120.0))
        );
    }

    #[test]
    fn the_unknown_sentinel_picks_tracks_with_no_artist_or_no_album() {
        let mut a = app();
        backend::sample::unknown_tracks(&mut a.backend);
        assert!(a.reload_library());
        let get = |title: &str| a.lib.tracks.iter().find(|t| t.title == title).unwrap();
        let no_album = get("Fixture no album");
        let with_album = get("Fixture with album");
        let no_artist = get("Fixture no artist");
        let matches = |f: &Filter, t: &Track| a.matching(f).iter().any(|r| r.id == t.id);
        let mut f = Filter {
            albums: vec![data::UNKNOWN.into()],
            ..Default::default()
        };
        assert!(matches(&f, no_album) && !matches(&f, with_album));
        f.albums.push("Some Album".into());
        assert!(matches(&f, no_album) && matches(&f, with_album));
        f.albums.clear();
        f.artists = vec![data::UNKNOWN.into()];
        assert!(matches(&f, no_artist) && !matches(&f, with_album));
        f.artists = vec!["Unknown artist".into()];
        assert!(!matches(&f, no_artist));
    }

    #[test]
    fn a_playlist_needs_a_filter_other_than_the_search_to_be_saved() {
        let mut a = app();
        let _ = a.update(Message::ToggleJam);
        let before = a.lib.playlists.len();
        // Nothing set, and only a search: refused with a notice, nothing is created.
        save_jam(&mut a);
        assert_eq!(a.lib.playlists.len(), before);
        assert!(a.notice.as_ref().is_some_and(|n| n.0.contains("Add a")));
        a.filter.q = "a".into();
        save_jam(&mut a);
        assert_eq!(a.lib.playlists.len(), before, "a search alone is not saved");
        // An artist (any of several, one of them the unknown sentinel) is enough, and is kept.
        a.filter.artists = vec![a.lib.tracks[0].artist.clone(), data::UNKNOWN.into()];
        save_jam(&mut a);
        assert_eq!(a.lib.playlists.len(), before + 1);
        assert!(a.reload_library());
        let p = a
            .lib
            .playlists
            .iter()
            .find(|p| p.filter.as_ref().is_some_and(|f| f.artists.len() == 2))
            .expect("both artists were stored");
        assert!(
            p.filter
                .as_ref()
                .unwrap()
                .artists
                .contains(&data::UNKNOWN.to_string())
        );
    }

    #[test]
    fn narrow_sort_and_timer_sheets_close_before_applying_their_actions() {
        use weave_player_lib::core::CollectionOrder;
        let mut a = app();
        let _ = a.update(Message::Resized(390.0, 844.0));
        let _ = a.update(Message::Open(Dest::Tracks));
        let _ = a.update(Message::OpenSheet(Some(Sheet::Sort)));
        let _ = a.view();
        let _ = a.update(Message::SheetAction(Box::new(Message::CollectionOrder(
            CollectionOrder::Rating,
        ))));
        assert!(a.sheet.is_none());
        assert_eq!(a.collection_order(), CollectionOrder::Rating);
        assert!(
            a.rows
                .windows(2)
                .all(|pair| pair[0].rating >= pair[1].rating)
        );
        let _ = a.update(Message::OpenSheet(Some(Sheet::Sleep)));
        let _ = a.view();
        let _ = a.update(Message::SheetAction(Box::new(Message::SleepTimer(Some(
            15,
        )))));
        assert!(a.sheet.is_none());
        assert_eq!(a.sleep_choice, Some(15));
        let _ = a.update(Message::ToggleJam);
        let _ = a.update(Message::JamFilters(true));
        let _ = a.view();
        let _ = a.update(Message::JamFilters(false));
        assert!(!a.jam_filters_open);
    }
    #[test]
    fn main_menu_sheet_closes_before_applying_settings_or_importing() {
        let mut a = app();
        let _ = a.view();
        let _ = a.update(Message::Resized(400.0, 800.0));
        let _ = a.update(Message::OpenSheet(Some(Sheet::Main)));
        let _ = a.view();
        assert_eq!(a.sheet, Some(Sheet::Main));
        let _ = a.update(Message::SheetAction(Box::new(Message::SetDensity(
            DensityPref::Compact,
        ))));
        assert!(a.sheet.is_none());
        assert!(a.compact());
        let _ = a.update(Message::OpenSheet(Some(Sheet::Main)));
        let _ = a.update(Message::SheetAction(Box::new(Message::ThemeMode(Some(
            iad::Scheme::Dark,
        )))));
        assert!(a.sheet.is_none());
        assert_eq!(a.theme_mode, Some(iad::Scheme::Dark));
        assert_eq!(a.theme().scheme, iad::Scheme::Dark);
        let _ = a.update(Message::OpenSheet(Some(Sheet::Main)));
        let _ = a.view();
        // Creating the picker task does not run the OS dialog in this test.
        let _ = a.update(Message::SheetAction(Box::new(Message::Import(
            import_picker::Mode::Files,
        ))));
        assert!(a.sheet.is_none());
        let _ = a.update(Message::OpenSheet(Some(Sheet::Main)));
        let _ = a.update(Message::OpenSheet(None));
        assert!(a.sheet.is_none());
    }

    #[test]
    fn imports_open_jam_with_a_composable_removable_recent_filter() {
        let mut a = app();
        let t = a.lib.tracks[0].clone();
        let _ = a.update(Message::JamArtistToggle(t.artist.clone()));
        let _ = a.update(Message::Imported(Ok(Default::default())));
        assert!(a.jam_open);
        assert_eq!(a.filter.imported_within_days, Some(7));
        assert_eq!(a.filter.artists.as_slice(), std::slice::from_ref(&t.artist));
        assert!(!a.rows.is_empty());
        assert!(a.matching(&a.filter).iter().all(|t| t.added_at > 1));
        let spec = a.filter.to_spec();
        assert_eq!(Filter::from_spec(&spec).imported_within_days, Some(7));
        let _ = a.view();
        let _ = a.update(Message::Resized(400.0, 800.0));
        let _ = a.update(Message::JamFilters(true));
        let _ = a.view();
        let _ = a.update(Message::JamRecent(false));
        assert!(
            a.matching(&a.filter)
                .iter()
                .any(|track| track.slug == t.slug)
        );
        assert_eq!(a.filter.artists, [t.artist]);
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
        let _ = a.update(Message::Imported(Ok(
            weave_player_lib::core::ImportReport {
                added: 2,
                ..Default::default()
            },
        )));
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
