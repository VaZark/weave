# Components

The frontend is the first application built on iced-libadwaita, so the aim is to find the components that more than one app would want, build those there, and keep only the music-specific ones here. Statuses come from the iced-libadwaita `DESIGN.md`, `docs/lib/layout.md` and `docs/lib/motion.md`; check the repository for the current state before building.

## Already in iced-libadwaita

| Need                                          | Component                                                     |
| --------------------------------------------- | ------------------------------------------------------------- |
| Filter, tag and settings dialogs              | `dialog_in_window`, `preferences_dialog`                      |
| Nested settings pages, artist and album pages | `navigation_view`                                             |
| Sidebar inside the wide layout                | `NavigationSplitView`, `SidebarList`                          |
| Library pills, 7 and 30 day switch            | `toggle_group`                                                |
| Usage cards, shelves of cards                 | `Card`, `boxed_list`                                          |
| Tag tree in settings                          | `ExpanderRow`                                                 |
| Artist avatars                                | `avatar`                                                      |
| Feedback ("Added to queue")                   | `Toast`, `toast_stack`                                        |
| Narrow-layout Now Playing                     | `bottom_sheet_in_window` (see the change below)               |
| Width classes for `adaptive`                  | `breakpoints::WidthClass`, `window::is_mobile`                |
| Layout-change motion, reduced motion          | `Trigger::Layout`, `animation::Tween`, `animation::enabled()` |

## Provided by iced-libadwaita once [#3](https://github.com/VaZark/iced-libadwaita/issues/3) lands

The frontend is designed on the assumption that these exist upstream, and does not carry its own copies.

- **`transition`:** the command and event messages, outcomes, deadline timer and sequencing (see [layout](layout.md#transitions)). Components report through it when their own animation ends.
- **`adaptive`:** the meta component that maps width classes to layouts and runs the close-then-open sequence when the choice changes. It assumes nothing about what a layout contains, so a layout may be a tab bar, a sidebar or anything else.
- **`action`:** a registry entry (label, icon, shortcut, enabled, badge) plus helpers that render it as a tab, a sidebar row, a header button or a menu item.

Until they land, the frontend can prototype them locally behind the same names and move to the upstream versions without changing its layouts or views. The prototype is also what the issue's plan asks for.

## Still to build in iced-libadwaita

These are general; they belong in the library, not in this application. Build them in this order: the filter page depends on the first four, and the narrow layout and header search depend on 6 and 7.

1. **`popover`:** anchored to a widget, closing on an outside click. Used by the filter button.
2. **Region-scoped bottom sheet:** the existing sheet covers the window; the jam sheet must cover only the content area and leave the player bar visible.
3. **`token_entry`:** an entry whose values are tokens, with autocomplete from a supplied list.
4. **`range_scale`:** a two-handle scale (`scale` has one handle). Used for the BPM range.
5. **`lazy_list` / `column_view`:** a virtualised list with sortable, hideable columns. A library of tens of thousands of tracks needs it, and it is the hardest piece.
6. **Bottom variant of `view_switcher` / `tab_bar`** for the narrow layout's tab bar.
7. **Header title slot that takes a widget,** so the title can turn into the search entry with its filter button.
8. **Horizontal shelf:** a scrolling row of cards. `carousel` supports only mouse drag.

## Music-specific, kept in this application

- Cover with a blurred backdrop (downscale and blur on the CPU, cached per cover).
- Transport buttons and the seek bar.
- Synchronized lyrics list.
- Application-owned symbolic transport, navigation and tag artwork lives in the `weave-icons` workspace crate. It retains Weave’s earlier stroke-based appearance while the toolkit handles rendering and system-themed window chrome.

## Not in the first version

The audio peaks visualiser, an equaliser, and anything for servers, radio or devices.
