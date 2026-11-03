# Weave UI

The design of the Weave GUI frontend: a separate application built with [iced](https://iced.rs) and [iced-libadwaita](https://github.com/VaZark/iced-libadwaita) that embeds the `weave-player-lib` library, or talks to another instance as a thin client. The decision and its alternatives are in [ADR 0008](../adr/0008-gui-frontend-iced-libadwaita.md).

Two goals shape it. It is the first real application built on iced-libadwaita, so a component that more than one app would want is built there, not here (see [components](components.md)). And it serves what Weave is for: organising music by the user's own tags, so the filter is the centre of the interface and the tags are the user's own words, never a fixed list.

## Pages

- [layout](layout.md): actions, triggers, views and layouts as separate things, the adaptive component that picks a layout, the first two layouts (narrow and wide), the library and player views, and the transition sequences.
- [jam](jam.md): the dynamic mode, its bottom sheet and the filter page that search shares.
- [home](home.md): recent jams, playlists and artists, and the 7 and 30 day usage summary.
- [components](components.md): what the frontend needs from iced-libadwaita, and what already exists there.

## Principles

- **The frontend owns no rules.** Filtering, playback, queue and playlist semantics live in the `weave-player-lib` library; the frontend renders state and sends commands. A thin client uses the same views over the remote endpoint, so every view works against either.
- **One filter UI.** Search and jam use the same filter page. A saved dynamic playlist is that same filter, stored.
- **Tags are the user's words.** The UI never lists a fixed set of keys. It shows the user's own vocabulary with counts, and offers autocomplete from it. Managing the vocabulary (rename, nest, merge) is a settings page, not part of the main navigation.
- **Playing is always from somewhere.** A play action plays what is listed (all tracks, an album, a playlist, an artist's discography, a jam). There is no context-free "play".
- **Actions, views and layouts are separate.** A trigger only fires an action, a view only renders state, and a layout only arranges them. An upstream `adaptive` component (iced-libadwaita#3) picks the layout from the width, so every layout can be built and judged on its own.
- **Transitions report back.** Closing or opening something takes a time budget and ends with an event, so the order of animations is managed rather than assumed.

## Vocabulary

A **jam** is a shuffled, filter-driven queue that keeps refilling. It is not the persisted playback **session** (the queue and its position) described in `goals.md`; starting a jam replaces the queue and uses the session like any other play action.

## Status

The first version is `apps/gui` (`cargo run -p weave-player-gui`). It runs on a fake library and a simulated player, so the UI can be judged before the engine is wired in: both layouts, Home, the track table, artists, albums, playlists, search with the filter popover, the jam sheet, Now Playing with synced lyrics and the queue. Development switches: `WEAVE_GUI_SIZE=420x760`, `WEAVE_GUI_START=jam|search|library|playing|queue|lyrics` and `WEAVE_GUI_SHOT=file.png` (screenshot and quit).

Our own widgets live in `apps/gui/src/components`, with their headless logic in `apps/gui/src/primitives`; generic ones (the synced lyrics list) carry no dependency on the app, so they can move upstream.

## Current design (supersedes the sidebar in [layout](layout.md))

- **One screen, no sidebar, no library pills.** Home is only what is recent: playlists (a jam saved with **Save as playlist** is a dynamic playlist, so there is no separate list of jams), recently played, albums and artists, each section with "View all". **Explore** (a header button, and a tab on narrow) starts with the links to every list and the usage summary, then Rediscover (old favourites from "It's been a while", and what you play most) and Explore (you might like), ending with direct links to all playlists, artists, albums and tracks. Recently played is reached from Home ("View all"). The two layouts differ only in the player (a bar, or a mini player and a tab bar) and small details.
- **Cards fit the width.** iced has this natively: `iced::widget::Grid::fluid(max_width)` gives as many columns as fit, and the cards fill their cells (`ui::autofit`).
- **The search field takes focus when it opens** (`FocusScope` with `trap(false)` and `initial_focus`, from iced-adaptive-design).
- **On desktop every header button is a toggle:** unselecting it returns to Home (Explore, Search, Jam; on narrow Explore and Jam are tabs, not header buttons). On narrow, tabs and Back apply.
- **Back is the previous place** (see [layout](layout.md#back)).
- **Jam is a page**, not a sheet. Its header button toggles it: pressed, the jam shows; pressed again, the page you were on returns. Filters are open by default and fold on request; the round play button sits right after them; the matches are a virtualised list.
- **Track lists are flat:** no surfaces, no separators, the whole row lights up on hover. A row is the picture, the title with its artist beneath it, the album (not on an album's own page), the duration, and a menu. Tempo and rating are not in the rows: tempo is in the jam's list and under **More info**; **Rate** and **Edit tags** are menu entries. **Compact mode** (Settings) drops the picture and uses one line for tracks (every list and the jam's), and turns the cards for albums, artists and playlists into tight rows with a small picture and the name beside it, several to a line, as a music app's compact library does.
- **Settings is a dialog** (the header gear; iad's `preferences_dialog` with a page of groups and rows): compact mode, and how many days before Explore resurfaces a highly rated track. It closes with its button or a click outside.
- **Menus put their icons (and checks) on the right edge**, after the label.
- **Now Playing and the queue animate both ways, and only animate.** iad's sheet entrance has no exit animation, so the app drives a small `Slide` (0.28 s, eased, skipped when animations are off) from iced's frame subscription, which runs only while something moves. Now Playing slides up over the page and back down. The queue is a push of the final layout: the now-playing bar rises from the bottom to the top, the page (or Now Playing, if it was open) is pushed up ahead of it, and the space below becomes the queue; closing runs it backwards. A non-scrolling `scrollable` gives the content unbounded room, so it is translated, not squeezed. The layouts at either end are unchanged.
- **The page gutter is enforced by code.** It is defined once, privately, in `ui::surface` (built on iad's `Container`): `scrolling` (a scrolling page, with the scroll bar at the window edge), `full`, `band`, `toolbar` and `row` (list rows inset so their hover surface starts on the gutter). Pages are built only from these, in both densities. A test fails if a page file mentions the constant or hard-codes a `.padding(..16..)`.
- **Now Playing is a sheet** that slides up over the page area (the whole window body on narrow). On wide it does not repeat the transport controls the player bar already has; on narrow it carries them. **The queue is page content, not a sheet:** it pushes the page out and the now-playing bar moves to the top of the page (the player bar on wide, the mini player on narrow), above "Up next". The playing track is not in the list, so it cannot be dragged or removed; "Up next" is only what follows it, always editable (delete, then the drag handle), with the sleep timer (and shuffle and repeat on narrow).
- **Detail pages** (playlist, artist, album) copy the usual music-app header: wide, the cover on the left, kind label, large title, description, facts, and an action row (play, options, a list/compact switch at the far end); narrow, the cover centred and one action row with the options at the start and the round play button at the end. An artist page adds the aggregate rating and a favourite toggle, Popular albums (best rated, then most in playlists), Latest albums, then all tracks.
- **Explore** uses play history: most listened artists and tracks, and "It's been a while": tracks rated 4+ not played for more than N days (90, 180 or a year, in Settings).

## Developing against a local iced-adaptive-design

The GUI pins a revision of iced-adaptive-design. To build against a sibling checkout that is ahead of it, create `.cargo/config.toml` (ignored by git) with a `[patch."https://github.com/VaZark/iced-libadwaita"]` entry pointing at `../iced-libadwaita`. The tempo picker is iad's `ComposedComponent` (a `BarChart::histogram` over a `range_slider`), so it needs a revision with the composed component.

## TODO

- **Karaoke mode:** large synced lyrics with vocal level, key and tempo shift, on Home and Now Playing. Needs the engine to duck or separate vocals.
- Play history and favourites in the engine (Home, the artist page and "It's been a while" use sample data).
