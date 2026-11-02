# ADR 0004: Keyed tags with an optional single parent, and the filter model

Status: accepted. Reversible while the schema is the pre-0.1 squashed one.

## Problem

Imported metadata sat in fixed columns while user tags were flat names, so filters could not express "any bachata
subgenre" or numeric ranges over descriptive values, and every new descriptive field meant a new column and a new
predicate.

## Decision

**Tags**

- A tag has an optional key, a value, a normalized lookup identity and at most one parent. `genre:bachata` and
  `practice` are equally valid; the colon is syntax, not hierarchy. Identity does not depend on the parent.
- Assignment of a tag to a track is explicit and flat: assigning `genre:bachata-traditional` does not also assign
  `genre:bachata`. Reparenting changes what a query reaches, never what is assigned.
- Multiple roots are allowed. Self-parenting and cycles are rejected in the same transaction as the change.
  Deleting a tag requires choosing `Detach` (children become roots) or `Delete` (descendants go too).
- Tags go on tracks, artists, albums and playlists: one vocabulary and one tree, with an assignment table per kind
  (`track_tags`, `artist_tags`, `album_tags`, `playlist_tags`). A filter only looks at the kind it is run against; a
  track does not inherit its artist's or album's tags.
- Schema: `tags(name, normalized_name, key, value, value_num, parent_id)` and `track_tags(track_id, tag_id)`, indexed
  on `parent_id`, the normalized name, `(key, value_num)` and `(tag_id, track_id)`. Descendants come from a recursive
  CTE; no closure table until benchmarks justify one.
- `bpm` and `year` values must be non-negative numbers. A value that parses as a number is also stored in
  `value_num`, so ranges are numeric and indexed, never lexicographic. Tag names cannot start with `=`.

**What is a tag**

- Tags hold descriptive metadata and user-defined tags, nothing else. Identifying data (title, artist, album), the
  source path, playback state and annotations (rating) are never tags (`agents.md`, invariant 10).
- Import maps `genre`, `year` and `bpm` to tags. Once imported they are ordinary tags: the library owns them, there is
  no origin or provenance column, and nothing re-syncs known tracks.

**Filters**

- `has` (the default) matches a tag and its descendants; `has_exact` matches explicit assignments only; exclude
  removes a tag and its descendants. All/any/none keep their boolean meaning, and groups combine with AND.
- `FilterSpec` has the flat `tag_all/any/none` lists (an `=` prefix means exact), keyed `tag_clauses` (operator, keys,
  values, exact; several keys mean "any of these keys"), and numeric `tag_range`s.
- CLI: `--tag-all`, `--tag-any` and `--tag-exclude` take bare names or `KEYS=VALUES`
  (`genre=bachata,sals`, `genre==bachata` for exact); `--tag-range KEY=MIN..MAX`.
- The genre, year and BPM predicates are gone. `--title`, `--artist` and `--album` stay as substring filters. The
  metadata columns remain for sorting and display.

## Alternatives

- **An unrestricted DAG or a separate hierarchy table:** more than one parent is not needed, and a strict tree keeps
  cycle checks and queries simple.
- **Provenance per assignment (`user` or `import`):** would let a future re-sync refresh derived tags without touching
  the user's. Dropped: import's job is to make sources identical, and nothing refreshes known tracks. Add it back if
  that ever changes.
- **Tags for everything, including artist and album:** every track would carry unique identifying tags. Rejected by
  the rule above.
- **A query expression language:** a non-goal; filters are typed parameters bound to SQL.

## Consequences

- Descendant lookup costs a recursive CTE per tag.
- Tracks carry `genre:`, `year:` and `bpm:` tags. The TAGS column skips the ones shown in their own column, and
  autoplay compares `bpm` and `year` by closeness instead of as shared tags.
- Genre matching is exact (or by parent), where the old `--genre` was a substring.
- Saved playlists that used the removed predicates lose those conditions; the schema is pre-0.1, so libraries are
  deleted and re-imported.
