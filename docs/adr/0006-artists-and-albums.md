# ADR 0006: Artists and albums as entities, and never merging by guess

Status: accepted.

## Problem

A library is more than a list of tracks: it holds artists and albums, and the user wants to see its shape (what an
artist is made of, how an album sits under an artist, who is really the same person). Artist and album were only
text columns on each track, so there was nothing to open, tag or correct, and two spellings of one artist were two
unrelated strings.

## Decision

- **Entities.** `artists` and `albums` are tables of their own, linked from `tracks` (`artist_id`, `album_id`), each
  with a slug ID and a profile. The metadata columns keep the text, so artist and album remain identifying data and
  never become tags (ADR 0004).
- **Linking at import.** A name is one artist when it is spelled the same, ignoring case and spacing; otherwise a
  new artist is made. An album belongs to its album artist, else the track's artist, and is unique per artist and
  title. Albums are linked to artist profiles this way, and `album link`/`unlink` fix it by hand.
- **No automatic dedup.** Names that merely look alike (punctuation, a leading "the") stay separate artists, because
  different people share names. `artist duplicates` suggests such pairs. Import asks about the pairs it just created
  when it runs in a terminal, and `--dedup-off` skips the questions. Answers are merge, keep separate (remembered in
  `artist_separate`, never asked again) or skip.
- **Merging** (`artist merge <from> <into>`) moves tracks, albums and tags, rewrites the track metadata to the
  surviving name, merges same-titled albums and keeps the old spelling as an alias so a re-import lands on the
  survivor. It is the only way two artists become one.
- **Tags on everything.** Artists, albums and playlists take the same tags as tracks (ADR 0004).
- **Views.** `track <id>`, `artist <id>` and `album <id>` show everything the library knows: figures over their
  tracks (length, years, BPM, ratings, when they entered the library), genre and tag distributions, albums and
  tracklists. `artist list` and `album list` filter by the tags on the artists or albums themselves.
- **Favorites are removed.** Ratings are the only user annotation on a track.

## Alternatives

- **Merge on a loose key automatically:** would silently fuse different people with similar names.
- **Ask on every import about every similar pair:** repetitive; answers are remembered and `--dedup-off` exists.
- **Make artist and album tags:** every track would carry unique identifying tags, against ADR 0004.
- **Inherit tags from artist and album into track filters:** hides where a tag came from and breaks `has_exact`.
  Possible later as an explicit option.

## Consequences

- Names are only grouped by exact spelling, so a person spelled two ways is two artists until merged; people who
  share a spelling are one artist until a split exists (not built).
- "Feat." credits are not split into several artists.
- Artists made only as album artists (such as "Various Artists") exist with no tracks of their own.
- Existing development libraries must be deleted and re-imported (the schema is still the pre-0.1 squashed one).
