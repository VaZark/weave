# Layouts

The frontend has no fixed shell. It is made of four separate things, so each can be designed, tried and replaced on its own.

| Thing       | What it is                                                                                        | Knows about                                   |
| ----------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| **Action**  | Something the user can ask for: go Home, open Jam, search, play all, open the queue.              | Its label, icon, shortcut and enabled state.  |
| **Trigger** | Any widget that shows an action: a tab, a sidebar row, a header button, a menu item.              | The action it fires, nothing else.            |
| **View**    | Content: Home, Tracks, an artist, the filter page, Now Playing, the queue.                        | App state in, actions and messages out.       |
| **Layout**  | Arranges triggers and hosts views for one window class (wide, narrow, and any other added later). | Which triggers go where and how views appear. |

The **adaptive** meta component picks the layout from the window width.

## Actions and triggers

An action is a value, not a button. It is registered once with its label, icon, shortcut, enabled state and an optional badge (the filter count on the search action, for instance), and every trigger renders from that one entry. The Jam action is a ✦ button in the wide layout's header and a tab in the narrow layout's tab bar; both fire the same `OpenJam`, and neither knows what happens next.

Pressing a trigger sends the action to the application. The application updates its state (the current destination, which overlay is open) and the layouts render that state. Layouts never hold navigation state, so switching layouts loses nothing: the page, the open jam sheet, the search text and the queue are all still there.

| Action                  | Wide layout                           | Narrow layout                               |
| ----------------------- | ------------------------------------- | ------------------------------------------- |
| Go Home                 | Sidebar row                           | Home tab                                    |
| Recently played         | Sidebar row                           | A shelf on Home                             |
| Tracks, Artists, Albums | Sidebar rows                          | Pills on the Library tab                    |
| Playlists               | Sidebar section                       | Pill on the Library tab                     |
| Search                  | Header 🔍                             | Search tab                                  |
| Open jam                | Header ✦                              | Jam tab                                     |
| Queue                   | Player bar button, opens a side panel | Toggle in Now Playing                       |
| Now Playing             | Player bar ⌃, expands in place        | Tap the mini player, expands to full height |

A trigger can show state as well as fire. The Jam tab is highlighted while the jam sheet is open, and the search button carries the filter badge.

## Views

A view is a function of application state. It sends actions and messages and has no idea how it is hosted: the jam sheet's filter page is the same view whether the layout presents it as a sheet over the content or at full height on a phone. A view may say how much space it wants; the layout decides where it goes.

### Library views

- **Tracks** lists every track in a sortable table (title, artist, album, tags, BPM, rating; columns can be hidden). Its **▶** plays everything listed, so with a search active it plays the filtered list. Double-clicking a row plays the same list from that row.
- **Artists** and **Albums** are grids. An artist page has **Play discography**, its albums, the tracks it is featured on and its profile figures. An album page has **Play** and the tracklist in disc and track order; clicking a track plays the album from it.
- **Playlists:** a playlist page has **Play**. A dynamic playlist shows its filter as a sentence ("romantic · spanish · under 100 BPM") and an Edit button that opens the filter page.
- **Tagging** is done from a row's menu or from Now Playing, in an Edit tags dialog (tracks, albums, artists and playlists alike). Dropping tracks onto a playlist adds them.
- **Settings → Tags** is the vocabulary: the tree of the user's tags with counts, and rename, nest and merge. It is a nested page of the settings dialog, not part of the main navigation.

### Now Playing

- A large cover over a blurred, darkened copy of itself, following light or dark mode.
- Synchronized lyrics as a scrolling list with the current line highlighted; tapping a line seeks to it. Plain lyrics scroll without highlight.
- The track's tags as editable chips, so a track can be re-tagged while it plays.
- A toggle between lyrics and queue.

### Search and jam

See [jam](jam.md). Search is a header mode in the wide layout and a tab in the narrow one; both use the same filter page.

## Adaptive

`adaptive` is a meta component provided by iced-libadwaita ([#3](https://github.com/VaZark/iced-libadwaita/issues/3)). It takes the window width, maps width classes to layouts and renders the chosen one.

```text
adaptive
  width < 768 px  →  NarrowLayout
  otherwise       →  WideLayout
```

- The thresholds are iced-libadwaita's width classes, so they follow the user's `[breakpoints]` configuration. A third layout (for a tablet, say) is one more entry in the map; no other layout changes.
- Layouts do not know about each other. Each takes the same inputs (application state, the action registry, the views) and can be built, run and judged alone, for instance by forcing it in a gallery or a test regardless of window size.
- The width can hover around a threshold while a window edge is dragged, so the choice needs a little hysteresis, or it would flip layouts every few pixels.
- Changing layout is not a cut. The adaptive component closes the old layout and opens the new one in order, using the transition protocol from [iced-libadwaita#3](https://github.com/VaZark/iced-libadwaita/issues/3).

## The two first layouts

### Narrow layout (phone)

```text
┌──────────────────────────────┐
│ Home                    ⚙    │
│                              │
│  (page content)              │
│                              │
├──────────────────────────────┤
│ [cover] Title – Artist  ⏯   │  mini player
├──────────────────────────────┤
│  ⌂     🔍      ☰   ✦ (jam)  │
└──────────────────────────────┘
```

- **Home** shows [recent jams, playlists, artists and usage](home.md).
- **Search** is the search entry with its filter button. Its empty state shows recent searches.
- **Library** has a row of pills (a toggle group) for Tracks, Artists, Albums and Playlists.
- **Jam** is a tab that fires the Jam action. It is not a page: the [jam sheet](jam.md) opens over the content area with the mini player still visible, and closing it returns to the tab the user was on.
- Tapping the mini player expands Now Playing to full height.

### Wide layout (desktop, tablet)

```text
┌────────────────┬─────────────────────────────────────────────┐
│ ☰              |                 Weave            🔍   ✦   ⋮│
├────────────────┼─────────────────────────────────────────────┤
│ ⌂ Home         │  Title        Artist     Album      BPM  ★  │
│ ◷ Recently     │  …                                          │
│    played      │                                             │
│ ────────────   │                                             │
│ Tracks         │                                             │
│ Artists        │                                             │
│ Albums         │                                             │
│ ────────────   │                                             │
│ Playlists      │                                             │
│  ⚡ Bachata 90s │                                             │
│  ☰ Road trip   │                                             │
├────────────────┴─────────────────────────────────────────────┤
│ [cover] Title – Artist  ⏮ ⏯ ⏭  ━━●━━ 1:12   🔀 🔁 🔊   ⌃      │
└──────────────────────────────────────────────────────────────┘
```

- **Sidebar:** Home and Recently played first, then Tracks, Artists, Albums, then the playlists (⚡ dynamic, ☰ manual). The sidebar holds no tags.
- **Header, right corner:** 🔍 Search and ✦ Jam.
- **Search:** firing the Search action turns the centred title into a search entry with a filter button; the button opens the filter popover (see [jam](jam.md#the-filter-page)). Escape or ✕ restores the title and clears the search and its filters. A badge on the filter button counts the active filters.
- **Jam:** the [jam sheet](jam.md) opens over the content area. The player bar stays visible and keeps playing.
- **Queue:** a full-height sheet that slides up over the window (in both layouts), scrollable, with drag to reorder. It does not share a pane with the lyrics.

On a phone the app is usually a thin client of the desktop instance, with an instance switcher in the settings.

## Transitions

Opening and closing is ordered with the transition protocol from [iced-libadwaita#3](https://github.com/VaZark/iced-libadwaita/issues/3): a target is told to `Close` or `Open` within a time, hard-closed by the coordinator if it overruns, and reports `Closed` or `Opened` with an outcome (`Early`, `OnTime` or `TimedOut`). Any outcome lets the next step start. The frontend's own sequences, with illustrative budgets:

| Event                     | Sequence                                                                                        |
| ------------------------- | ----------------------------------------------------------------------------------------------- |
| Window crosses 768 px     | `Close(Wide, 30 ms)` → `Open(Narrow, 30 ms)` (run by `adaptive`)                                |
| Start a jam               | `Close(JamSheet, 200 ms)` → show the first track in the player bar                              |
| Escape from header search | `Close(SearchEntry, 100 ms)` → restore the title                                                |
| Open the jam from search  | `Close(SearchPopover, 100 ms)` → `Open(JamSheet, 200 ms)`, with the search filters carried over |

The frontend's own views (jam sheet, queue panel, header search) report through the same events when their animation ends. A new sequence replaces the steps that have not started and lets the step in flight finish, so nothing is left half-closed.

## Back

Back goes to the previous place, not to a fixed parent. Opening a page from somewhere (an artist from Home, an album from a search, an artist from an album's track menu) pushes where you were, including an active search and its text; the chevron pops it. A top-level destination (a sidebar row, a tab, a Library pill) starts the history over, so no chevron shows there.

## Now Playing

The cover; when lyrics are switched on they take the cover's place (a scrolling list that follows the song and seeks when tapped). Shuffle, repeat, lyrics and queue are toggles under the controls.
