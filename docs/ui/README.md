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
