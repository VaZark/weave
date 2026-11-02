# ADR 0007: Dynamic playlists that draw on other playlists

Status: accepted.

## Problem

A dynamic playlist is a query over the whole library. Often the real intent is narrower: "romantic Spanish tracks, but only from my road trip", or "from either of these two playlists, the quick ones". Repeating the first playlist's rules in the second drifts out of date, and a manual playlist's hand-picked tracks cannot be expressed as a filter at all.

## Decision

- A filter has `sources`, a list of playlists to draw on (`--from <playlist>`, repeatable). Without sources the whole library is the candidate set, as before.
- **Sources are ORed, then ANDed with the filter.** A track must be in at least one source, and the playlist's own rules (tags, BPM, year, rating, text) then apply on top.
- **A manual source** limits the candidates to its tracks, so the filter runs over a small, hand-picked set.
- **A dynamic source** contributes what it matches right now, evaluated live, including its own sources. A dynamic source with a limit contributes exactly the tracks its sort and limit keep (up to 500), because a "top 15" is not the same as everything that matches its rules.
- **Saved playlists keep their sources as playlist IDs.** Sources must exist when saved, and a playlist cannot draw on itself, directly or through others; an update that would make a loop is rejected. Nesting is limited to eight levels.
- `search` and `play random` take `--from` too. `playlist update --from` replaces the sources, and `--clear-sources` removes them.
- Manual playlists stay explicit lists with no filters and no sources, so the two models are not mixed: a dynamic playlist is still a query, and it uses a manual one only as a set of tracks.

## Alternatives

- **AND across sources:** a track in every source. Rarely what is wanted from a handful of playlists, and a single source can already say it with its own rules.
- **Copy the source's rules into the new playlist:** drifts out of date and cannot reach a manual playlist.
- **Snapshot the source's tracks when saving:** loses the point of a dynamic playlist, which refreshes as the library changes.

## Consequences

- A dynamic playlist's membership can change when a source it draws on changes, including a manual one that is edited.
- A source with a limit above 500 cannot be drawn on, since its tracks are bound one by one into the query.
- There is no playlist delete yet; once there is, deleting a playlist that others draw on needs a rule (refuse, or detach).
