# ADR 0004: Keyed tags with an optional single parent, and the filter model

Status: accepted. Reversible while the schema is the pre-0.1 squashed one.

## Problem

Imported metadata sat in fixed columns while user tags were flat names, so filters could not express "any bachata subgenre", and descriptive labels beyond what the file says had nowhere to live.

## Decision

**Tags**

- A tag has an optional key, a value, a normalized lookup identity and at most one parent. `genre:bachata` and `practice` are equally valid; the colon is syntax, not hierarchy. Identity does not depend on the parent.
- Assignment of a tag to a track is explicit and flat: assigning `genre:bachata-traditional` does not also assign `genre:bachata`. Reparenting changes what a query reaches, never what is assigned.
- Multiple roots are allowed. Self-parenting and cycles are rejected in the same transaction as the change. Deleting a tag requires choosing `Detach` (children become roots) or `Delete` (descendants go too).
- Tags go on tracks, artists, albums and playlists: one vocabulary and one tree, with an assignment table per kind (`track_tags`, `artist_tags`, `album_tags`, `playlist_tags`). A filter only looks at the kind it is run against; a track does not inherit its artist's or album's tags.
- Schema: `tags(name, normalized_name, key, value, parent_id)` and `track_tags(track_id, tag_id)`, indexed on `parent_id`, the normalized name, `key` and `(tag_id, track_id)`. Descendants come from a recursive CTE; no closure table until benchmarks justify one.
- A tag may carry a look for frontends: an opaque `icon` (a key or path, meaning is the frontend's) and a `#rrggbb` `color`. Both are optional, belong to the tag (not to an assignment) and never affect identity, the tree or filters. The tag engine is the vendored `weaver` crate.
- Tags are text. Numbers (year, BPM, duration, rating) are columns of the track with their own bounds, not tags. Tag names cannot start with `=`.

**What is a tag**

- Tags hold the genre and the user's own descriptive tags, nothing else. Identifying data (title, artist, album), the source path, playback state and annotations (rating) are never tags (`agents.md`, invariant 10).
- Import maps only the genre to a tag (`genre:`). Once imported it is an ordinary tag: the library owns it, there is no origin or provenance column, and nothing re-syncs known tracks.

**Filters**

- `has` (the default) matches a tag and its descendants; `has_exact` matches explicit assignments only; exclude removes a tag and its descendants. All/any/none keep their boolean meaning, and groups combine with AND.
- `FilterSpec` has the flat `tag_all/any/none` lists (an `=` prefix means exact), keyed `tag_clauses` (operator, keys, values, exact; several keys mean "any of these keys").
- CLI: `--tag-all`, `--tag-any` and `--tag-exclude` take bare names or `KEYS=VALUES` (`genre=bachata,sals`, `genre==bachata` for exact).
- The genre predicate is gone (use the tag). `--title`, `--artist` and `--album` stay as substring filters, and BPM and year keep their own inclusive bounds on the track's columns.

## Alternatives

- **An unrestricted DAG or a separate hierarchy table:** more than one parent is not needed, and a strict tree keeps cycle checks and queries simple.
- **Provenance per assignment (`user` or `import`):** would let a future re-sync refresh derived tags without touching the user's. Dropped: import's job is to make sources identical, and nothing refreshes known tracks. Add it back if that ever changes.
- **Tags for everything, including artist and album:** every track would carry unique identifying tags. Rejected by the rule above.
- **Numeric tags with range queries:** an indexed number column on tags, validated for `bpm` and `year`. Dropped: numbers already live in the track's columns, and a user's own numbers are better as a rating or a plain tag.
- **A query expression language:** a non-goal; filters are typed parameters bound to SQL.

## Consequences

- Descendant lookup costs a recursive CTE per tag.
- Tracks carry a `genre:` tag. The TAGS column skips the one the GENRE column shows.
- Genre matching is exact (or by parent), where the old `--genre` was a substring.
- Saved playlists that used the removed genre predicate lose that condition; the schema is pre-0.1, so libraries are deleted and re-imported.
