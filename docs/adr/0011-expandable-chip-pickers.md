# Expandable chip pickers

ChipGroup line limits must leave an expand chevron after the last visible chip on
the final clamped line, rather than a separate Show more row. Measure the actual
chip widgets and reserve the chevron's width on that line, hiding the trailing chip
when needed. Expanded groups show every chip and end with an upward chevron.
An ordinary height clamp cannot reserve that inline space; use a small wrapping
widget that keeps stable child state while excluding hidden chips from drawing,
input, operations and overlays. No count or text-width estimates live in Weave.

The opt-in ChipGroup::expandable builder retains configurable max_lines (three by
default), independent controlled expansion and controlled search/selection. Short
lists and searches omit the chevron. Weave resets presentation state when an editor
opens. The existing fixed-clamp API remains available. This is a reversible
presentation change with no persistence, schema or filter-semantic changes.
