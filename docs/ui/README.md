# Weave UI

The design of the Weave GUI frontend: a separate application built with [iced](https://iced.rs) and [iced-libadwaita](https://github.com/VaZark/iced-libadwaita) that embeds the `weave-player-lib` library, or talks to another instance as a thin client. The decision and its alternatives are in [ADR 0008](../adr/0008-gui-frontend-iced-libadwaita.md).

Two goals shape it. It is the first real application built on iced-libadwaita, so a component that more than one app would want is built there, not here (see [components](components.md)). And it serves what Weave is for: organising music by the user's own tags, so the filter is the centre of the interface and the tags are the user's own words, never a fixed list.

## App-wide action style

- **Prefer icons over text for actionable UI elements throughout the app.** Use meaningful icon buttons with tooltips and accessible action names for commands, toolbar actions, dialog/sheet actions, and repeated controls. Apply this consistently in shared components and on every page. Choose icons that describe the action (for example, dice for Surprise me and shuffle for playback order); add an appropriate icon when one is missing rather than substitute an unrelated symbol. Keep text for content, headings, field labels, named choices, and menu items where it identifies what the user is choosing. Use a text action only when an icon would make the action ambiguous.

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

The first version is `apps/gui` (`cargo run -p weave-player-gui`). It runs on the engine (`weave-player-lib`): the library, the queue and the transport come from the library database (`--db <path>`, `$WEAVE_DB`, or the CLI's default `~/.local/share/weave/library.db`), and sound comes from the embedded player (feature `audio`, on by default). `weave-player dev seed` fills a database with sample tracks. Both layouts, Home, the track table, artists, albums, playlists, search with the filter popover, the jam sheet, Now Playing with synced lyrics and the queue. Development switches: `WEAVE_GUI_SIZE=420x760`, `WEAVE_GUI_START=jam|search|library|playing|queue|lyrics` and `WEAVE_GUI_SHOT=file.png` (screenshot and quit).

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

The GUI and `weave-icons` iced adapter pin iced-adaptive-design to `52dff3836dd27b05eb44d5e0636a6da3ad710535` in the root workspace manifest. To build against a sibling checkout that is ahead of it, create `.cargo/config.toml` (ignored by git) with a `[patch."https://github.com/VaZark/iced-libadwaita"]` entry pointing at `../iced-libadwaita`. The tempo picker is iad's `ComposedComponent` (a `BarChart::histogram` over a `range_slider`), so it needs a revision with the composed component.

Tag pickers initially show at most three complete lines; artist and album pickers
show at most two. A downward chevron after the last chip on the final clamped line expands the picker;
an upward chevron after the expanded chips restores the limit. Each picker expands independently. A chevron appears only if the actual wrapped
content exceeds the limit, including after a search or window resize. Searching
and expanding never change selected values. Reopening a filter or track-tag editor
starts collapsed. Track-tag choices scroll inside the dialog so Save/Cancel remain
accessible when expanded. `App::chip_line_limit` configures tag pickers;
`App::name_chip_line_limit` configures artists and albums.

For reproducible software-rendered review images, run
`WEAVE_CHIP_REVIEW_DIR=/tmp/weave-chip-review cargo test -p weave-player-gui --no-default-features render_chip_filters`.
This covers wide/narrow, collapsed/expanded, a short search result, and the track-tag
dialog; toolkit tests additionally click expansion controls and hidden/visible chips.

The tag icon inside the search entry switches between all tags and category browsing.
Category chips are never line-clamped. Select a named key to show its tags (including
nested tags), or **Custom** for unkeyed tags. **Categories** returns to the category
list; the tag icon returns to all tags. Search is available in every mode, matches
full tag names, and finds categories through their matching child tags. A child-tag
query stays active on entering a category; a category-only search clears when it
would otherwise hide all its children. Navigating
never changes selections in live filters, saved-filter drafts, or track-tag editors.
The same engine discovery APIs supply every picker.

For manual review, the development start states `WEAVE_GUI_START=tag-categories`,
`tag-genre` and `tag-custom` open the corresponding Jam filter views. Automated
behavior tests run with
`cargo test -p weave-player-gui -p weave-player-engine --no-default-features -- --skip render_chip_filters`;
visual review is performed by the user.

The main menu's **Import files…** and **Import folder…** actions open the system
chooser directly. Files mode imports the selected files; folder mode recursively
imports the confirmed folder. The empty-library button opens the folder chooser.
Cancellation imports nothing. There is no in-app import browser or confirmation
page. Importing runs off the UI thread; partial successes reload the library and
show per-path issues, and clean imports show a summary. System file/folder modes
were chosen during user review because the portal cannot return the currently
browsed folder when no file is selected.

All collection pages use search from the window header. Artist and album collections use the
Recent playlist layout (image beside name), with a flat surface, in a responsive grid on wide layouts
and full-width list rows on narrow layouts. Artist and album entries omit track counts;
Home and artist-detail album groups use the same presentation. Explore sections are
collapsible and retain their open state while navigating. Library links come first,
followed by one Explore section containing About me and Rediscover, then
most listened tracks/albums/artists. The
sample Usage widget is removed. Rediscover's filter dialog edits inactivity days,
ranking, ratings and the same tag/artist/album/tempo/exclusion controls as search.

About me shows measured listening overview metrics, rolling day/week/month/year
or custom UTC calendar ranges, and shared filter controls. It has no play/save
controls. Empty history stays empty. Statistics load on a worker and update while
open; charts remain deferred. Playlist editing retains fields not exposed by the
GUI, and membership/order come from the engine rather than a partial GUI projection.

The toolkit supplies a neutral ProfileHeader with image/name/description and optional
after-name, top-right and controls slots. Weave owns its media controls composition;
the full old composition remains a gallery example rather than a toolkit export.

Exclusion entries accept comma-separated tag names; escape a comma or backslash
inside a tag name with a backslash. Loaded definitions are encoded and decoded
without losing those characters. Selecting Any in the BPM presets explicitly clears
stored open-ended bounds; leaving the BPM control unchanged preserves exact bounds.

Artist pages use the same collapsible section dividers as Explore for Popular albums,
Latest albums and All tracks. Sections start expanded and remember their state per
artist while the application is running.

The bottom player places the favourite button immediately after the track information.
On wide layouts the progress scrubber sits beside previous/play/next controls and
has a minimum width of 200 px, including its time labels. Playback controls and
scrubber form a centered group, with equal space for metadata and actions on either side.
Fullscreen and Now Playing toggle icons are removed; clicking the track information
still opens Now Playing, and the fullscreen keyboard shortcut remains available.
The narrow mini-player places its progress indicator below the track and controls.

All tracks/artists/albums/playlists use the detail-page header and background wash.
Play is always at the far left, with secondary actions grouped on the right in both
layouts. Sort offers: Recent, Release date,
Added date or Rating. Home's recent collections share the engine's Recent order
(latest played, with latest import fallback). Collections use member tracks for
release/import/rating sorts; ratings average only rated members and unknowns sort
last. Each All page remembers its sort during the session. Detail-page sorting is
temporary and preserves saved playlist order. Jam's sort precedes its icon-only
Save as playlist action and stays separate from the saved query. Sort-icon choices
are ephemeral playback rules, held only in GUI session state. They order the current
matches after saved limits, without rewriting playlist membership, saved order or
filters. Starting sorted playback follows that rule instead of random shuffle;
choosing Shuffle afterward clears the temporary sort. Existing saved shuffle
preferences remain intact; All tracks also exposes the player shuffle toggle.

Search is opened by the window-header icon on collection, detail and Jam pages.
While active, a toolkit search bar replaces the page's media header inline, following
the gallery's header/search swap pattern. There is no search dialog or bottom sheet.
Typing narrows only the current page's existing data. Playlist search composes
FilterSpec.sources with the query; other pages use engine filtering over their
current candidate IDs. Jam filters, source membership, saved limits and ordering
remain intact. Closing search or pressing Escape clears its transient text and
restores the header and complete current collection. Filter pickers retain their
own tag/name lookup.

Narrow menus always use bottom sheets, including sort and sleep timer. Desktop
menus remain anchored dropdowns. Filter and import-result sheets are bounded to
80% of the window height, leaving the page visible behind a dismissible scrim.
Weave does not use full-page menu or drawer presentations.

Boolean and radio menu choices use checkmarks for selection, without On/Off state
text, consistently in desktop dropdowns and narrow bottom sheets.

Selecting the active sort again cycles descending, ascending, then deselected.
The selected option shows its direction; deselection restores the collection's
normal order. Unknown dates/ratings stay last in either direction. The tag-filter
Clear action uses a reset-arrow icon rather than a trash can.

Unavailable icon actions use the theme's disabled opacity, including playback,
queue clear, filter reset and Jam save. Disabled buttons remain non-interactive.

Filter actions use the toolkit Indicator to overlay a tiny accent dot on the icon's
top-right corner when active, without a numeric badge or extra layout width. Sorting
retains its descending/ascending/deselected cycle. Manual playlists have an icon-only
order editor: pencil enters edit mode, checkmark saves, cross cancels, and every
control has a tooltip. Rows have drag handles plus up/down actions. The draft is
separate from playback and is saved atomically; stale drafts are rejected rather
than overwriting concurrent changes. Saved-filter playlists keep engine-generated
membership and cannot enter this manual-order editor.

Jam keeps its match count immediately after Play. The empty-filter hint uses the
same text size and reserved line height as a medium chip, so the first chip does
not move the surrounding header. Artist/album filter choices retain their positions
when selected instead of moving to the front.

All playlists has a + action immediately before Sort for creating an empty manual
playlist. Creation is a full-screen page in both layouts, reusing the playlist
filter editor, name form and validation, then opening the shared playlist detail page. Manual playlists add a searchable library-track picker
(dialog wide, bounded bottom sheet narrow), with +/trash membership actions, and
Remove from playlist in each track's regular menu. Changes persist immediately and
leave library tracks and playback intact. Reordering retains its separate cancellable
draft; membership editing is unavailable during that draft. Dynamic playlists retain
filter-based membership and their existing actions.

Manual playlist creation saves optional addition rules and starts with no tracks.
These rules use the same tag, artist, album, tempo and exclusion semantics as dynamic
playlists, but constrain only the tracks offered by Add tracks. They never silently
populate the playlist, hide existing curated tracks or change their saved order.
Manual/dynamic kind is explicit in the GUI snapshot, independent of whether rules
are present. Edit filter remains available alongside ordinary curation controls.

Saving changed manual rules previews the curated tracks disqualified by the new
rules. If any would be removed, a confirmation dialog shows their exact count on
both desktop and narrow layouts (never a bottom sheet). Cancel returns to editing
without saving rules or membership. Confirm atomically saves the rules and removes
those playlist memberships, retaining the library tracks and playback. Concurrent
membership or eligibility changes invalidate the review and require a fresh preview.

Removing a selected tag also opens this confirmation when the affected count is
zero. Relaxing an AND rule can disqualify no tracks; the dialog reports zero rather
than implying that removing a tag necessarily removes members.

Filter tag pickers start with categories. The tag icon switches to all tags
and back to categories, without changing selections. The leading back chevron opens
the category list. Category chips (`Custom`, `lang`, `genre`, and other keys)
use their category colors and icons; user-defined keys get stable colors.

Individual tag pills inherit their category color in suggestions, selected input
pills, and page summaries, including Custom and user-defined tag keys.

Recent and All artist, album, and playlist collections use the shared
`components::collection::CollectionLayout` for both entries and their layout.
The shared `CollectionFlow` primitive flows into a grid in wide mode (260 px
cards, 220 px compact), and full-width rows in narrow mode. Home recent artists,
albums, and playlists explicitly use cards at both widths. All playlists uses the same artwork, counts, and card rendering
as Recent playlists. Collection search and ordering only change the items.

Themes opens a dialog on wide layouts and a bottom sheet on narrow layouts.
Import themes and choose Desktop or an imported theme there; selecting a theme
previews it immediately while keeping the chooser open. Compact density remains
a separate setting. The compact player window and its sizing controls are removed.

Filter panels share `components::responsive_dialog` at the window root: a bounded
dialog wide and a full-screen sheet narrow. `components::filter_editor` owns the
common header, scrolling body, slim scrollbar and fixed icon action row. The
active Jam, page, or playlist-draft context supplies fields/messages and actions.
Resizing keeps the same filter; live changes remain live and draft changes still
require Save. Header close, Escape and desktop backdrop all use the context's
close action. Actions use icon buttons with tooltips.

Application icons come from `weave-icons` (`crates/icons`), preserving the earlier 16px stroke artwork instead of replacing it with desktop-theme drawings. Its optional `iced` feature provides cached `view`/`colored` rendering and conversion to the toolkit’s `IconSource`, so menus, chips, avatars and toggle buttons share the same artwork and inherit their current foreground. Existing stored tag/playlist keys keep their mappings. Toolkit-owned chrome still follows installed themes, and the GUI uses `utils::detect_theme` / `utils::follow_theme` for desktop changes.

The **Themes** dialog (or its narrow-screen sheet) includes a **Dark mode** switch above the theme choices. It forces dark appearance for the current Weave session; turning it off resumes the selected skin’s scheme or the desktop preference. It does not write desktop settings, change the selected skin, or affect playback or density.
