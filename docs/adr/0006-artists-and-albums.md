# ADR 0006: Artists and albums as entities, and never merging by guess

Status: accepted.

## Problem

A library is more than a list of tracks: it holds artists and albums, and the user wants to see its shape (what an artist is made of, how an album sits under an artist, who is really the same person, who only shares a name). Artist and album were only text columns on each track, so there was nothing to open, tag or correct. Guessing that two names are one person is destructive, because different people share names.

## Decision

- **Entities.** `artists` and `albums` are tables of their own, linked from `tracks` (`artist_id`, `album_id`), each with a slug ID and a profile. The metadata columns keep the text, so artist and album remain identifying data and never become tags (ADR 0004). Artist names are not unique.
- **Featured artists.** `Song feat. B` in the artist field or the title makes B a credit of its own (`track_featured`, linked to an artist), and the track keeps its primary artist. Duos such as `Simon & Garfunkel` are not split.
- **Import creates, never collapses.** An artist found by an import is always a new entity, never attached to one already in the library. Within one run a name shared by several tracks is one artist, otherwise every track would have its own. An album belongs to its album artist, else the track's artist, and is unique per artist and title; `album link`/`unlink` fix it by hand. No alias is kept: nothing later depends on a spelling.
- **One duplicate check for artists, albums and tracks.** It finds things that look alike: artists by name (the very same name too), albums by title with matching or unknown artists, tracks by title and artist with lengths within three seconds. After an import the check asks about the pairs involving what the import added when it runs in a terminal; `--disable-dedup-check` skips it, and `dedup list`/`dedup ask` do it later. Answers are merge, keep separate (remembered in `not_duplicates`, never asked again) or skip.
- **Merging** (`dedup merge <kind> <from> <into>`) is the only way two become one. Artists: tracks, featured credits, albums and tags move, same-titled albums fold together and track metadata spells the surviving name. Albums: tracks and tags move. Tracks: the other file stays attached (a track then has several files and the oldest plays), and tags, a missing rating, featured credits, queue spots and playlist places move.
- **Splitting** (`artist split`) makes a new artist, with the same name if wanted, from chosen tracks and albums, for people who share a name; the two are marked as different. `artist slug` edits an artist's ID.
- **Tags on everything.** Artists, albums and playlists take the same tags as tracks (ADR 0004).
- **Views.** `track <id>`, `artist <id>` and `album <id>` show everything the library knows: figures over their tracks (length, years, BPM, ratings, when they entered the library), genre and tag distributions, featured credits, albums and tracklists. `artist list` and `album list` filter by the tags on the artists or albums.
- **Favorites are removed.** Ratings are the only user annotation on a track.

## Alternatives

- **Attach to an existing artist with the same spelling:** silently fuses different people who share a name.
- **Merge on a loose key automatically:** the same, worse.
- **Keep aliases so a re-import lands on the survivor:** it would guess again on every import.
- **Dedup only artists:** albums and tracks pile up copies from re-imported folders just the same.
- **Make artist and album tags:** every track would carry unique identifying tags, against ADR 0004.
- **Inherit tags from artist and album into track filters:** hides where a tag came from and breaks `has_exact`.

## Consequences

- Importing an artist again makes another artist of the same name; the check asks to merge them each time, or `--disable-dedup-check` defers it to `dedup ask`.
- The public ID of an artist can now change (`artist slug`), an exception to "slugs never change" made on the user's explicit request. Album and track IDs made from the old name keep it.
- A merged track's second file is kept but only the oldest plays; there is no fallback if it goes missing.
- Artists made only as album artists (such as "Various Artists") exist with no tracks of their own.
- Existing development libraries must be deleted and re-imported (the schema is still the pre-0.1 squashed one).
