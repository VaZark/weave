# ADR 0004: Unified tags and tree-based matching

Status: Proposed

## Context

Weave currently stores imported metadata in fixed columns and user tags in a separate tag table. Smart playlist filters use exact tag names, plus dedicated metadata predicates. We want one extensible descriptive vocabulary without making source identities, file paths, playback state or user-protected annotations into tags.

## Decision

- A tag has a stable internal ID, optional key (for example `genre`, `bpm`, `period`), value, normalized lookup identity and at most one optional parent tag.
- An unkeyed tag such as `practice` is valid. A keyed tag such as `genre:bachata-traditional` is equally valid. The colon is display/query syntax, not a hierarchy separator.
- Each tag has zero or one parent. Multiple roots are allowed. Cycles are rejected. A separate hierarchy table and unrestricted DAG are not needed.
- Track-to-tag assignment is always explicit and flat. Assigning `genre:bachata-traditional` does not also assign `genre:bachata`.
- Tag identity is independent of the parent relationship. Reparenting a tag changes query membership but does not rewrite assignments.
- `has(tag)` includes the tag and descendants; `has_exact(tag)` matches only explicit assignments; `has_none(tag)` excludes the tag and descendants. All/any/none groups retain their current boolean semantics.
- Creating a tag accepts an optional parent; changing or clearing its parent is supported. Prevent self-parenting and indirect cycles transactionally. Deleting a parent must use an explicit detach-or-delete policy.
- Blessed keys such as `genre`, `bpm`, `year` and `artist` are recognized by the importer and query planner; arbitrary keys remain valid. Type-aware numeric range comparisons must be indexed and validated, not performed by lexicographic string comparison.
- Metadata import maps supported provider fields into tags through the core, recording origin/provenance per track assignment. Re-imports must not overwrite user annotations or manually applied tags.
- Preserve canonical track identity, source mappings, and operational fields as structured relational data. Do not remove the existing metadata columns until import, sorting, queries and compatibility are migrated and tested.

## Examples

```text
genre:bachata
├── genre:bachata-traditional
└── genre:bachata-sensual

practice
remix
period:90s
bpm:90
```

A track assigned only `genre:bachata-traditional` matches `has(genre:bachata)` but not `has_exact(genre:bachata)`. A query with `has(genre:bachata)` and `has_none(genre:bachata-traditional)` excludes that track.

## Storage and performance

Extend the existing `tags` table with optional key, typed/indexable value where applicable, and nullable `parent_id REFERENCES tags(id)`. Keep `track_tags` as the explicit many-to-many mapping. Index `tags(parent_id)`, normalized tag identity and `track_tags(tag_id, track_id)`. Start with SQLite recursive CTEs for descendants. Avoid closure tables and caches until benchmarks justify them.

For a strict tree, each tag belongs to one parent at most. Reparenting must check ancestry and reject cycles in the same transaction.

## Implementation sequence

1. Introduce the tag domain model, normalization, blessed-key registry and schema changes, with integrity tests.
2. Expose create/list/reparent/delete tag operations through the `weave` library API and CLI. Keep the UI thin.
3. Add hierarchy-aware `has`, `has_exact` and `has_none` query predicates, with saved playlist serialization and regression tests for all/any/none.
4. Normalize supported imported metadata into tags with provenance and protection for user edits. Keep compatibility reads for existing metadata fields until consumers migrate.
5. Add typed comparisons, indexes and benchmarks for larger libraries.

## Tradeoffs

A single-parent tree cannot express overlapping taxonomies directly; independent root trees and multiple explicit track tags cover the initial use cases. Recursive CTEs add work at query time, but preserve simple writes and a small schema. The current pre-0.1 schema is fingerprinted, so schema changes require a fresh library/re-import under the project's existing policy.
