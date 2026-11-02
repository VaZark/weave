# Home

Home is the first page on both layouts (the Home tab in the narrow layout, the first sidebar entry in the wide one). It answers "what was I listening to, and how".

```text
Good evening                                   [ 7 days | 30 days ]
┌ Listening ───────┐ ┌ By source ───────────────┐ ┌ Top tags ──────┐
│ 12h 40m · 214    │ │ Jams 46% · Playlists 31% │ │ romantic · …   │
│ tracks           │ │ Albums 15% · Artists 8%  │ │ spanish · …    │
└──────────────────┘ └──────────────────────────┘ └────────────────┘

Recent jams        ▸ [romantic · spanish · <100 BPM  ▶] [calm · french ▶] …
Recent playlists   ▸ [cover][cover][cover] …
Recent artists     ▸ [avatar][avatar][avatar] …
```

## Usage summary

A toggle switches the window between the last **7 days** and the last **30 days**, and all three cards follow it.

- **Listening:** time listened and number of tracks played.
- **By source:** how the listening divided between jams, playlists, albums, artists and single tracks.
- **Top tags:** the tags on the tracks played most in the window, whatever their keys. The UI hard-codes no key; a user who tags only with `mood:` and `lang:` sees those, one who tags with `energy:` and `wedding` sees those.

## Shelves

- **Recent jams:** a card per jam, showing its filters as a sentence and a ▶ that starts it again. The card menu has **Save as playlist**.
- **Recent playlists** and **recent artists:** cards that open the playlist or the artist page. The Recently played entry in the wide sidebar is the full, scrollable version of the same history.

## What the engine has to provide

`goals.md` does not describe these yet. Home cannot be built until they exist in the `weave` library, which keeps the rule that the frontend owns none of them.

- **Play history:** a row per play with the time and where it came from (jam, playlist, album, artist, single track), kept on the instance. The queue and the playback session alone cannot answer "last 7 days".
- **Jam history:** the filters of each jam with the time last used, so a jam can be started again and saved as a playlist.
- **Windowed figures:** listening time, track count, the split by source and the most played tags between two dates, computed by the library so a thin client gets them over the same endpoint.
