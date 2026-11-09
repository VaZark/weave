# Complete AND/OR filter groups

Filters must express `(romantic AND slow) OR (party AND fast)`. Tag-any alone cannot
express this because tempo and the other conditions must stay inside each branch.

Extend FilterSpec with child `groups` and a typed `group_join`, AND by default.
Own fields are shared constraints, ANDed with the combined children. Each child
uses the same filter predicates and may contain further groups. Empty groups are
ignored. Sort and limit belong only to the final result, before paging; child
sort/limit fields are rejected. Validate all children, limiting expressions to
eight levels and 64 nodes. Existing serialized definitions default to no children
and AND, preserving their semantics. No database migration is needed.

Storage composes parameterized branch predicates with the existing tag and metadata
queries. Playlist source resolution, source-loop checks, dependency discovery,
detachment and clear-sources traverse every group. Manual sources still contribute
curated membership; manual addition rules use the grouped query only to offer
candidates. Entity tag searches apply grouping to the entity's own tags, retaining
the existing rule that track metadata filters do not apply to artists/albums.

The CLI accepts repeated `--filter-group` JSON objects and `--group-join and|or`:

```sh
weave-player search --group-join or \
  --filter-group '{"tag_all":["mood:romantic"],"bpm_max":90}' \
  --filter-group '{"tag_all":["mood:party"],"bpm_min":120}'
```

Ordinary flags such as `--rating-min 4` apply to both branches. The same flags work
when creating dynamic playlists and manual addition rules. Updates containing
groups replace the grouped expression; updates omitting groups preserve it. CLI
ruleset output includes every group as compact, shell-quoted JSON. Default AND and
invalid/nested rules are covered by parser and engine tests.

This keeps one engine filter model instead of adding a CLI-only expression parser
or repurposing tag-any for non-tag predicates. It adds JSON nesting and SQL branch
composition but no new dependency or schema. Group authoring in the GUI is deferred
at the user's request; no GUI controls are changed in this step.
