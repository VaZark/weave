# Flat tag filter blocks

The tag picker needs selection chips and the CLI needs readable AND/OR combinations.
Keep this in `vendor/weaver`, where tag normalization, validation and SQL already live.
Do not add an expression editor or expose nested tag groups.

`TagFilter.blocks` is a list of AND blocks, each a list of OR alternatives. Each
alternative has the existing all/any/exclude/keyed conditions; nested blocks are
rejected. Empty alternatives and blocks are ignored after normalization. Shared
conditions remain ANDed with the complete expression. `FilterSpec.tag_blocks`
passes these through to Weaver for every owner kind; BPM remains a track predicate.
This is additive serialized data with defaults and needs no schema migration.

CLI `--or-group` starts an alternative; `--and-group` starts the first alternative
of another AND block. Only tag flags belong to these alternatives. Other flags,
including BPM anywhere in the command, constrain the complete result:

```sh
weave-player search --or-group --tag-any genre:zouk,genre:bachata --tag-all mood:romantic --or-group --tag-all lang:tamil,tag:romantic,90s --bpm-max 90
weave-player search --or-group --tag-all mood:romantic --or-group --tag-all mood:calm --and-group --tag-all lang:tamil --or-group --tag-all lang:french
```

The second example means `(romantic OR calm) AND (Tamil OR French)`.
The GUI plus commits current selected tags as an any-tag alternative, clears the
selection, and shows a removable chip followed by a comma inside the tag input.
Category navigation changes presentation only. Recent-import and exclusion authoring
are removed from this dialog; existing saved conditions remain preserved.
