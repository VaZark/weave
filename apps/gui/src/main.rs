//! Weave's GUI frontend: iced with iced-adaptive-design (`iad`). See `docs/ui`.
//!
//! The UI comes first: it runs on a fake library (`data.rs`) and a simulated player, so every
//! view and both layouts can be judged before the engine is wired in. The application state
//! (destination, search, jam, queue) lives here; the layouts in `ui.rs` only render it, so a
//! window that crosses the breakpoint loses nothing.

mod components;
mod data;
mod icons;
mod ui;
mod ui_dialogs;
mod ui_lists;
mod ui_player;

use std::time::{Duration, Instant};

use data::{Filter, Library, Playlist, Track};
use iad::adaptive::{Adaptive, Layouts};
use iad::breakpoints::WidthClass;
use iad::primitives::sorting::{SortState, cmp_str};
use iad::primitives::synced::{self, Line};
use iad::primitives::virtualize::Viewport;
use iad::transition::Event;
use iad::widget::{ReorderEvent, ReorderState};
use iced::{Subscription, Task};
use iced_adaptive_design as iad;
use weaver::bulk::{self as bulk_tags, Change, Plan};

type Element<'a> = iad::Element<'a, Message>;

const JAM_LIST: f32 = 240.0;

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
    // Resizing and the transitions between layouts.
    Resized(f32),
    Tick(Instant),
    Transition(Event<Layout>),
    Window(iad::window::Action),
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
    ToggleCompact,
    SetCompact(bool),
    OpenSettings,
    CloseSettings,
    WhileDays(u32),
    ToggleFavorite(String),
    SleepTimer(Option<u32>),
    ListScroll(Viewport),
    RateTrack(usize),
    SetRating(usize, u8),
    CloseRate,
    InfoTrack(usize),
    CloseInfo,
    QueueRemove(usize),
    NowPlaying(bool),
    ToggleQueue,
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
    PlayTrack(usize),
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
    BulkOpen,
    BulkAddToggle(String),
    BulkMode(String, Mode3),
    BulkTarget(String, String),
    BulkSave,
    BulkCancel,
    EditToggleTag(String),
    SaveTrack,
    CancelTrack,
    /// Narrow layout: open or close the filters above the matches.
    JamFilters(bool),
    Back,
    Describe(String),
    EditDescription(bool),
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
    Queue(ReorderEvent),
    Progress,
    // Development: `WEAVE_GUI_SHOT=file.png` saves a screenshot of the window and quits;
    // `WEAVE_GUI_START=jam|search|library|artists|popover|playlist|dialog|artist|album|playing|queue|lyrics` opens that state first.
    Shoot,
    Shot(iced::window::Screenshot),
}

/// The track whose tags are being edited in the dialog.
#[derive(Debug, Clone)]
pub struct TrackEdit {
    pub id: usize,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode3 {
    Keep,
    Delete,
    Replace,
}

/// One tag already on the tracks being edited, and what to do with it.
#[derive(Debug, Clone)]
pub struct BulkRow {
    pub tag: String,
    pub count: usize,
    pub mode: Mode3,
    pub target: Option<String>,
}

/// Editing the tags of every track of an album or artist at once.
#[derive(Debug, Clone)]
pub struct BulkEdit {
    pub title: String,
    pub ids: Vec<usize>,
    pub add: Vec<String>,
    pub rows: Vec<BulkRow>,
}

impl Player {
    /// What comes after the track playing now, as an always-editable list. The playing track is
    /// not in it: it is shown on its own, so it cannot be dragged or removed.
    fn sync_up_next(&mut self) {
        let from = self.current.map_or(0, |c| c + 1).min(self.queue.len());
        self.up_next = editing_queue(self.queue[from..].to_vec());
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
}

pub struct App {
    pub lib: Library,
    pub adaptive: Adaptive<Layout>,
    pub dest: Dest,
    pub searching: bool,
    pub query: String,
    pub filter: Filter,
    /// Narrows the tag chips of the filter page as you type.
    pub tag_query: String,
    pub jam_open: bool,
    pub now_playing: bool,
    pub queue_open: bool,
    pub show_lyrics: bool,
    pub history: Vec<Place>,
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
    pub compact: bool,
    pub settings_open: bool,
    /// "It's been a while" on Home: rated highly, not played for this many days.
    pub while_days: u32,
    pub favorites: std::collections::HashSet<String>,
    /// Seconds left on the sleep timer.
    pub sleep_left: Option<f32>,
    /// The minutes chosen in the sleep-timer menu (`None`: off).
    pub sleep_choice: Option<u32>,
    /// The window of the main track list (virtualised) and where it starts.
    pub list_vp: Viewport,
    pub bulk: Option<BulkEdit>,
    /// Descriptions of detail pages, by `desc_key`, and whether the open one is being edited.
    pub descriptions: std::collections::HashMap<String, String>,
    pub editing_desc: bool,
    pub desc_expanded: bool,
    /// The jam sheet's matches as a virtualised list: the window of rows built, and where it
    /// starts in the full list.
    pub jam_filters_open: bool,
    /// The playlist the jam was last saved as, and the filters it was saved with.
    pub jam_saved: Option<(usize, Filter)>,
    pub jam_vp: Viewport,
    rng: u64,
}

pub const LYRICS: &str = "lyrics";

impl App {
    fn new() -> (Self, Task<Message>) {
        let lib = Library::fake();
        let queue: Vec<usize> = (0..lib.tracks.len()).collect();
        let mut app = Self {
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
            queue_open: false,
            show_lyrics: false,
            history: Vec::new(),
            sort: SortState::new(),
            rows: Vec::new(),
            player: Player {
                queue,
                up_next: editing_queue(Vec::new()),
                current: Some(3),
                playing: false,
                pos: 42.0,
                shuffle: false,
                repeat: false,
            },
            lyrics: Vec::new(),
            lyrics_of: None,
            lyric_line: None,
            days: 7,
            draft: None,
            edit_track: None,
            rate_track: None,
            info_track: None,
            compact: false,
            settings_open: false,
            while_days: 180,
            favorites: std::collections::HashSet::new(),
            sleep_left: None,
            sleep_choice: None,
            list_vp: Viewport::new(480.0),
            bulk: None,
            descriptions: std::collections::HashMap::new(),
            editing_desc: false,
            desc_expanded: false,
            jam_filters_open: true,
            jam_saved: None,
            jam_vp: Viewport::new(JAM_LIST),
            rng: 0x9e37_79b9_7f4a_7c15,
            lib,
        };
        for (i, p) in app.lib.playlists.iter().enumerate() {
            app.descriptions
                .insert(format!("playlist:{i}"), p.description.clone());
        }
        app.player.sync_up_next();
        app.refresh();
        let cmds = app.adaptive.resize(window_size().0, Instant::now());
        let mut tasks = vec![app.dispatch(cmds)];
        if let Ok(start) = std::env::var("WEAVE_GUI_START") {
            for m in match start.as_str() {
                "jam" => vec![Message::ToggleJam],
                "jam-filters" => vec![Message::ToggleJam, Message::JamFilters(true)],
                "search" => vec![Message::SearchOpen(true), Message::SearchText("a".into())],
                "library" => vec![Message::Go(Dest::Tracks)],
                "home-end" => vec![
                    Message::Go(Dest::Home),
                    Message::ListScroll(Viewport::new(480.0)),
                ],
                "explore" => vec![Message::Go(Dest::Explore)],
                "compact-home" => vec![Message::SetCompact(true)],
                "compact-albums" => vec![Message::SetCompact(true), Message::Go(Dest::Albums)],
                "compact-artist" => vec![Message::SetCompact(true), Message::Go(Dest::Artist(1))],
                "settings" => vec![Message::OpenSettings],
                "compact" => vec![Message::Go(Dest::Tracks), Message::ToggleCompact],
                "artists" => vec![Message::Go(Dest::Artists)],
                "popover" => vec![Message::SearchOpen(true)],
                "playlist" => vec![Message::Go(Dest::Playlist(0))],
                "playlist-in" => vec![Message::Open(Dest::Playlist(0))],
                "edit-track" => vec![Message::Go(Dest::Tracks), Message::EditTrack(3)],
                "bulk" => vec![Message::Go(Dest::Album(2)), Message::BulkOpen],
                "describe" => vec![Message::Go(Dest::Artist(1)), Message::EditDescription(true)],
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
        if std::env::var_os("WEAVE_GUI_SHOT").is_some() {
            tasks.push(Task::perform(
                tokio::time::sleep(Duration::from_millis(
                    std::env::var("WEAVE_GUI_DELAY")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(2500),
                )),
                |_| Message::Shoot,
            ));
        }
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
        self.bulk = None;
        self.edit_track = None;
        self.editing_desc = false;
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
        let mut items = self.player.queue.clone();
        let Some(cur) = self.player.current else {
            return self.play(vec![id], 0);
        };
        let at = if next {
            (cur + 1).min(items.len())
        } else {
            items.len()
        };
        items.insert(at, id);
        self.player.queue = items;
        self.player.sync_up_next();
    }

    pub fn track(&self, id: usize) -> &Track {
        &self.lib.tracks[id]
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
                Dest::Recent => self.lib.tracks.iter().rev().take(14).cloned().collect(),
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

    fn play(&mut self, ids: Vec<usize>, start: usize) {
        self.player.queue = ids;
        self.player.current = Some(start);
        self.player.pos = 0.0;
        self.player.playing = true;
        self.player.sync_up_next();
    }

    fn step(&mut self, forward: bool) {
        let n = self.player.queue.len();
        let Some(cur) = self.player.current else {
            return;
        };
        if n == 0 {
            return;
        }
        let next = if forward {
            cur + 1
        } else {
            cur.wrapping_sub(1)
        };
        self.player.pos = 0.0;
        if next < n {
            self.player.current = Some(next);
        } else if forward && !self.player.repeat {
            self.player.playing = false;
        } else {
            self.player.current = Some(if forward { 0 } else { n - 1 });
        }
        self.player.sync_up_next();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let now = Instant::now();
        let cmds = match message {
            Message::Resized(w) => self.adaptive.resize(w, now),
            Message::Tick(t) => self.adaptive.tick(t),
            Message::Transition(e) => self.adaptive.report(e, now),
            Message::Window(a) => {
                return iced::window::oldest().and_then(move |id| a.perform(id));
            }
            Message::Go(d) => {
                self.history.clear();
                self.show(d)
            }
            Message::Open(d) => {
                self.history.push(Place {
                    dest: self.dest,
                    searching: self.searching,
                    query: self.query.clone(),
                });
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
            Message::SetCompact(on) => {
                self.compact = on;
                Vec::new()
            }
            Message::OpenSettings => {
                self.settings_open = true;
                Vec::new()
            }
            Message::CloseSettings => {
                self.settings_open = false;
                Vec::new()
            }
            Message::ToggleCompact => {
                self.compact = !self.compact;
                Vec::new()
            }
            Message::WhileDays(d) => {
                self.while_days = d;
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
            Message::QueueRemove(i) => {
                // `i` counts in the "Up next" list, which starts after the playing track.
                let at = self.player.current.map_or(0, |c| c + 1) + i;
                if at < self.player.queue.len() {
                    self.player.queue.remove(at);
                }
                self.player.sync_up_next();
                Vec::new()
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
                self.play(ids, start);
                Vec::new()
            }
            Message::PlayAll(ids) => {
                if !ids.is_empty() {
                    self.play(ids, 0);
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
            Message::BulkOpen => {
                let title = self.page_title();
                let ids: Vec<usize> = self.rows.iter().map(|t| t.id).collect();
                let rows = bulk_tags::census(self.rows.iter().map(|t| t.tags.as_slice()))
                    .into_iter()
                    .map(|(tag, count)| BulkRow {
                        tag,
                        count,
                        mode: Mode3::Keep,
                        target: None,
                    })
                    .collect();
                self.bulk = Some(BulkEdit {
                    title,
                    ids,
                    add: Vec::new(),
                    rows,
                });
                Vec::new()
            }
            Message::BulkAddToggle(t) => {
                if let Some(b) = &mut self.bulk {
                    toggle(&mut b.add, t);
                }
                Vec::new()
            }
            Message::BulkMode(tag, m) => {
                if let Some(r) = self
                    .bulk
                    .as_mut()
                    .and_then(|b| b.rows.iter_mut().find(|r| r.tag == tag))
                {
                    r.mode = m;
                }
                Vec::new()
            }
            Message::BulkTarget(tag, to) => {
                if let Some(r) = self
                    .bulk
                    .as_mut()
                    .and_then(|b| b.rows.iter_mut().find(|r| r.tag == tag))
                {
                    r.target = Some(to);
                }
                Vec::new()
            }
            Message::BulkSave => {
                if let Some(b) = self.bulk.take() {
                    let plan = Plan {
                        add: b.add,
                        changes: b
                            .rows
                            .into_iter()
                            .map(|r| {
                                let change = match (r.mode, r.target) {
                                    (Mode3::Delete, _) => Change::Delete,
                                    (Mode3::Replace, Some(to)) => Change::Replace(to),
                                    _ => Change::Keep,
                                };
                                (r.tag, change)
                            })
                            .collect(),
                    };
                    for id in b.ids {
                        if let Some(t) = self.lib.tracks.get_mut(id) {
                            bulk_tags::apply(&mut t.tags, &plan);
                        }
                    }
                }
                self.refresh();
                Vec::new()
            }
            Message::BulkCancel => {
                self.bulk = None;
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
                if let Some(p) = self.history.pop() {
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
            Message::EditDescription(on) => {
                self.editing_desc = on;
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
                self.player.playing = !self.player.playing;
                Vec::new()
            }
            Message::Next => {
                self.step(true);
                Vec::new()
            }
            Message::Previous => {
                if self.player.pos > 3.0 {
                    self.player.pos = 0.0;
                } else {
                    self.step(false);
                }
                Vec::new()
            }
            Message::Seek(p) => {
                self.player.pos = p;
                Vec::new()
            }
            Message::Shuffle => {
                self.player.shuffle = !self.player.shuffle;
                Vec::new()
            }
            Message::Repeat => {
                self.player.repeat = !self.player.repeat;
                Vec::new()
            }
            Message::Queue(e) => {
                self.player.up_next.apply(e);
                // The list is always being edited: a drop is committed at once, into the queue
                // after the playing track.
                self.player.up_next.submit();
                self.player.up_next.edit();
                let keep = self
                    .player
                    .current
                    .map_or(0, |c| c + 1)
                    .min(self.player.queue.len());
                self.player.queue.truncate(keep);
                self.player
                    .queue
                    .extend_from_slice(self.player.up_next.items());
                Vec::new()
            }
            Message::Shoot => {
                return iced::window::oldest()
                    .and_then(iced::window::screenshot)
                    .map(Message::Shot);
            }
            Message::Shot(shot) => {
                if let Some(path) = std::env::var_os("WEAVE_GUI_SHOT") {
                    let size = shot.size;
                    if let Some(img) =
                        image::RgbaImage::from_raw(size.width, size.height, shot.rgba.to_vec())
                    {
                        let _ = img.save(path);
                    }
                }
                return iced::exit();
            }
            Message::Progress => {
                if let Some(left) = &mut self.sleep_left {
                    *left -= 0.25;
                    if *left <= 0.0 {
                        self.sleep_left = None;
                        self.sleep_choice = None;
                        self.player.playing = false;
                    }
                }
                self.player.pos += 0.25;
                if let Some(t) = self.now()
                    && self.player.pos >= t.secs as f32
                {
                    self.step(true);
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

    /// The height of a row in every track list.
    pub fn row_h(&self) -> f32 {
        if self.compact { ROW_COMPACT } else { ROW }
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
            iad::widget::follow_synced_text(LYRICS, line, self.lyrics.len())
        } else {
            Task::none()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            iced::window::resize_events().map(|(_, s)| Message::Resized(s.width)),
            self.adaptive.subscription(Message::Tick),
        ];
        if self.player.playing {
            subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::Progress));
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

/// A queue that is always in edit mode: its rows can be dragged and removed at any time.
fn editing_queue(items: Vec<usize>) -> ReorderState<usize> {
    let mut q = ReorderState::new(items);
    q.edit();
    q
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
    // Shared desktop behavior: ~/.config/iced-adaptive-design/desktop.conf (+ system files).
    iad::config::install(iad::config::Config::load());
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(window_size().0, window_size().1),
            ..iad::window::settings()
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
        App::new().0
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
        assert!(a.history.is_empty());
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
            a.history.is_empty(),
            "a top-level destination clears the history"
        );
    }

    #[test]
    fn compact_is_one_setting_for_every_list() {
        let mut a = app();
        let normal = a.row_h();
        let _ = a.update(Message::ToggleCompact);
        assert!(a.row_h() < normal);
        // the jam's list uses the same height
        let _ = a.update(Message::ToggleJam);
        assert!(a.jam_open);
        assert_eq!(a.row_h(), ROW_COMPACT);
        let _ = a.update(Message::ToggleJam);
        assert!(!a.jam_open, "the jam button toggles the page");
    }

    #[test]
    fn the_playing_track_is_not_in_up_next_and_the_rest_is_always_editable() {
        let mut a = app();
        let cur = a.player.current.unwrap();
        let playing = a.now().map(|t| t.id).unwrap();
        // Up next is what follows the playing track; the playing one is not in it.
        assert_eq!(
            a.player.up_next.items().len(),
            a.player.queue.len() - cur - 1
        );
        assert!(!a.player.up_next.items().contains(&playing));
        assert!(a.player.up_next.is_editing());
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
        assert!(a.player.up_next.is_editing());
        // removing the first upcoming row never touches the playing track either
        let _ = a.update(Message::QueueRemove(0));
        assert_eq!(a.now().map(|t| t.id), Some(playing));
        assert_eq!(a.player.queue[cur], playing);
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
    fn the_settings_dialog_changes_the_global_settings() {
        let mut a = app();
        let _ = a.update(Message::OpenSettings);
        assert!(a.settings_open);
        let _ = a.update(Message::SetCompact(true));
        let _ = a.update(Message::WhileDays(365));
        assert!(a.compact);
        assert_eq!(a.while_days, 365);
        let _ = a.update(Message::CloseSettings);
        assert!(!a.settings_open);
        assert!(a.compact, "closing keeps what was set");
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
}
