# 0004: Keyed tags with an optional single parent

## Status

Accepted.

## Context

Imported metadata sat in fixed columns while user tags were flat names, so filters could not express
"any bachata subgenre" or numeric ranges over descriptive values.

## Decision

- A tag has an optional key, a value, a normalized identity and at most one parent. Identity does not depend on the
  parent; reparenting rewrites no assignments. Cycles are rejected in the same transaction as the change.
- Track-to-tag assignment is explicit and flat. Imported tags are plain tags with no origin: import's job is to make sources
  identical, and the library owns the data afterwards. `has` follows
  descendants with a recursive CTE, `has_exact` does not.
- Numeric values live in `value_num`, indexed by `(key, value_num)`; `bpm` and `year` are validated as numbers.
- Deleting a tag requires `Detach` (children become roots) or `Delete` (descendants go too).
- Tags are for descriptive metadata and user-defined tags only. Identifying data (title, artist, album), source
  mappings, file paths, playback state and annotations (favorite, rating) stay structured, never tags. Import maps only
  `genre`, `year` and `bpm`.
- Metadata columns stay the source for sorting and the existing predicates until those migrate.
- `FilterSpec.tag_clauses` (key, op, values) are ANDed; the flat `tag_all/any/none` lists remain.

## Consequences

Descendant lookup costs a recursive CTE per tag; no closure table until benchmarks justify one. Imports only tag new tracks; there
is no re-sync of existing ones yet. If a re-sync or enrichment ever refreshes tags on known tracks, it will need a way
to leave the user's own edits alone, and that is the time to add provenance back.
