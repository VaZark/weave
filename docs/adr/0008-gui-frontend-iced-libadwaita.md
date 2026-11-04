# ADR 0008: The GUI frontend is built with iced and iced-libadwaita

Status: accepted. Reversible: the frontend is a separate project over the `weave-player-lib` library, so the toolkit can change without touching the engine.

## Problem

`goals.md` named GPUI as the GUI toolkit, and the first design pass borrowed from GTK players. The author maintains [iced-libadwaita](https://github.com/VaZark/iced-libadwaita), a GNOME-style component library for iced, and wants Weave's frontend to be the application that exercises it, so that components several apps would need end up standardised there.

## Decision

- **The frontend is an iced application using iced-libadwaita.** It is a separate project that embeds the `weave-player-lib` library (or acts as a thin client of another instance), as `goals.md` already requires of GUIs. The design is in [docs/ui](../ui/README.md).
- **No fixed shell.** Actions, triggers, views and layouts are separate, and an `adaptive` meta component picks a layout from the width. The first two layouts are a narrow one with a Spotify-style bottom tab bar (Home, Search, Library, Jam) and a wide one with a sidebar (Home and Recently played first) and Search and Jam in the header. Each layout can be built and judged alone.
- **Transitions are commanded, bounded and reported.** A layout or overlay is told to open or close within a time, the coordinator hard-closes it at the deadline, and it reports `Early`, `OnTime` or `TimedOut`, so animations can be put in a set order ([transitions](../ui/layout.md#transitions)).
- **Jam is a bottom sheet, not a page,** over the content area with the player bar visible. It uses the same filter page as search; the 🎲 button replaces the current filters, and a jam always shuffles and continues at the end of the queue.
- **Tags are not main navigation.** They are filtered through the filter page and managed in a settings page.
- **General components go in iced-libadwaita,** among them `adaptive`, `transition` and `action` (designed in [iced-libadwaita#3](https://github.com/VaZark/iced-libadwaita/issues/3), which the frontend assumes will be provided upstream); music-specific ones stay in the application ([components](../ui/components.md)).

## Alternatives

- **GPUI** (the Hummingbird route): native Rust and fast, but unrelated to the author's component work, and it has no GNOME look.
- **GTK4 and libadwaita through gtk-rs** (Tributary, Gapless, Vinyl): mature widgets, including a column view and virtualised lists. Rejected because the point is to dogfood iced-libadwaita; it is the fallback if the missing components prove too costly.
- **Plain iced with no component library:** more work for a less consistent result, and no dogfooding.

## Consequences

- iced-libadwaita says it may have rough edges and API churn. The frontend pins a version and treats a missing component as a task for the library.
- The largest gap is a virtualised, sortable list; it is built first (see [components](../ui/components.md)).
- `adaptive`, `transition` and `action` are assumed to arrive upstream. Until they do, the frontend prototypes them locally under the same names, so layouts and views do not change when the upstream versions replace them. If the upstream design diverges, the [UI docs](../ui/README.md) follow it.
- Home needs **play history** and **jam history** in the engine, which `goals.md` does not yet describe. They must be added there (and the rule that the frontend computes nothing kept) before Home is built.
- The word "session" already means the persisted queue and position, so the dynamic mode is called a **jam** everywhere.
