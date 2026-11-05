# Jam

A jam is a dynamic listening mode: pick some filters, press Start, and Weave plays the matching tracks shuffled, and keeps going with similar ones. Starting a jam is the same as `play` with a filter, `shuffle on` and `queue end continue`, so it needs nothing the engine does not have, apart from remembering jams (see [home](home.md#what-the-engine-has-to-provide)).

## Opening it

- **Wide layout:** the ✦ button in the header.
- **Narrow layout:** the Jam tab.
- Both fire the same Open jam action and open a **bottom sheet** that covers the content area completely and leaves the player bar (or mini player) visible, so what is playing keeps playing while the next jam is set up. On a phone the sheet is full height. A grab handle, Escape and a swipe down close it.

```text
┌──────────────────────────────────────────────────────────────┐
│                          ━━━                                 │
│  [mood:romantic ✕][lang:spanish ✕]  add tag or filter…       │
│                                                              │
│  Suggestions                         BPM  ○━━━━━━○  60–100   │
│   mood ▸  romantic 312 · calm 204 …   Genre  [latin ▾]       │
│   lang ▾  spanish 410 · french 88 …                          │
│   energy ▸ high 150 · low 96 …                               │
│                                                              │
│  [ 🎲 Surprise me ]                       [ Start · 142 ]    │
└──────────────────────────────────────────────────────────────┘
```

(The keys in the suggestions are the user's own; `mood`, `lang` and `energy` are examples.)

## The jam page

Search is part of Jam. The page has the look of an Album or Playlist page: a **header** with the title, the **active filters** as chips (each with a cross that clears it: a tag, the tempo, the artist, the album, the generic search), and a row of **icon buttons**: the round **Play**, **Tag filters** and **Search**, each with a tooltip and a badge counting its active filters. At the far corner are the match count and **+ Save as playlist**. Under the header comes the sortable table of matches (Title, Album, BPM, Time).

- **Tag filters** opens the filter page in a **dialog** (wide) or a **full-screen bottom sheet** (narrow). It applies live, and its header has a **Surprise me** icon and a clear icon, and Done.
- **Search** opens a row of three entries: **Artist**, **Album** and **Anything** (a generic search over title, artist and album).
- **Save as playlist** stores a real dynamic playlist from the tags, the tempo, the artist and the album. **The generic search ("Anything") is not saved**, and the notice says so when it has text; its chip also says it is not saved.

## The filter page

The filter page is one component with one state: tag tokens, genre and a BPM range, which is the typed filter the library already has (`FilterSpec`; no expression language). It adapts to the space its host gives it.

| Host                           | Used for                           | Primary action               |
| ------------------------------ | ---------------------------------- | ---------------------------- |
| Popover under the search entry | Narrowing the list being looked at | Applies live as it is edited |
| Bottom sheet                   | Setting up a jam                   | **Start · N**                |

- **Token entry:** tags and filters become tokens inside the entry. Typing autocompletes from the user's own vocabulary; a BPM bound can be typed (`bpm<100`) or set on the range.
- **Tag toggles:** the vocabulary as chips (the tag's icon on its colour, and its name) that switch on and off, with a find-as-you-type field above them. A chip also matches the tags under it. The live count shows a combination narrowing before it is committed. The vocabulary, icons and colours are managed in Settings → Tags.
- **Tags elsewhere:** Now Playing shows a track's tags as an icon on the tag's colour plus its value (a tag without an icon of its own gets the default of its key: genre, mood, language…). Track lists do not show tags; they are edited from a track's menu (**Edit tags and rating…**), and from an album's or artist's menu for all its tracks at once: add tags to every track, and for a tag already there, keep, delete or replace it where it matches.
- **Detail pages** (playlist, artist, album) are not searches: a back chevron, the title, a space for a description, the facts, then a round play button over the tracks. A dynamic playlist has a settings menu whose Edit filter opens the filter page in a modal dialog that closes only with Cancel or Save.
- **Live count:** the number of matching tracks, in the Start button and in the filter button's badge.
- **Moving between hosts:** the jam sheet can open seeded with the current search filters, and the popover has a "Start jam with these" link that opens the sheet. Searching for romantic Spanish tracks and then playing them as a jam is one tap.
- A saved dynamic playlist opens in the filter page for editing, and "Save as playlist" stores the current filters as a dynamic playlist.

## Surprise me

The 🎲 button picks filters at random from the user's own vocabulary: one or two tags, sometimes a BPM band, never a combination that matches nothing.

- **It replaces** whatever is in the entry. Each press clears the tokens and fills in a new set, so rerolling never piles filters up.
- **Undo:** after a roll the button offers ↶ for a moment, and Ctrl+Z restores the previous tokens, so an accidental press does not lose a hand-typed set.
- It exists only in the jam sheet. In the search popover, filters narrow a list, and replacing them at random would only be confusing.

## Starting

The round **play button** (the same as on a track list, with no label), right after the filters, replaces the queue with the matching tracks, turns **shuffle on** (a jam always shuffles) and **end of queue to continue**, so the queue refills with similar tracks (shared tags, BPM near the queue's average) instead of stopping. The sheet closes (a [transition](layout.md#transitions): `Close(JamSheet)`, then the player bar shows the first track, so the change is seen) and Now Playing shows the first track.

A jam is not a playlist. It is remembered as recent jams on Home, and the queue header has **Save as playlist**, which stores the jam's filters as a dynamic playlist.
