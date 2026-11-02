# Weave

A local-first music library for organising your own music by **mood**, filtering it by **language** and by **beat**, and keeping it all on your own machines. It is a Rust library you can embed in a player, and `weave-player`, a command-line player built on it.

## Why

Streaming services sort music by what is popular. I wanted to sort mine on my own terms, so Weave lets you invent your own tags and build dynamic playlists out of them.

A tag is a label you make up, with an optional key: `mood:romantic`, `lang:spanish`, `energy:high`, `wedding`. The keys and values are yours, and they can be nested: put `lang:spanish` and `lang:french` under `lang:romance`, and asking for `lang:romance` finds both. The genre is read from your files at import and you can refine it the same way, and the beat is each track's own BPM.

A dynamic playlist is a saved query over those tags, so it stays current as you tag more music. The one I started this for is mood plus language, and you can combine any tags you have.

```bash
# my own terms: romantic music sung in Spanish, not too fast
weave-player playlist create "Slow Spanish nights" --tag-all mood:romantic,lang:spanish --bpm-max 100 --sort bpm

# any tags work, in any combination
weave-player playlist create "Wedding, upbeat" --tag-all wedding --tag-exclude mood:sad --bpm-min 100
weave-player playlist create "Dance floor" --tag-any mood=party,energetic --tag-exclude live --bpm-min 110

# build on other playlists: a manual one limits the candidates, a dynamic one contributes what it matches now
weave-player playlist create "Slow Spanish, upbeat end" --from "Slow Spanish nights" --from "Road trip" --bpm-min 95

# tag as you listen, on tracks, artists, albums and playlists alike
weave-player add --tag mood:romantic,lang:spanish
weave-player play playlist slow-spanish-nights --shuffle
```

## What else it does

- Plays local files with a persistent queue, shuffle, repeat, a sleep timer, synchronized lyrics and OS media keys.
- Gives artists and albums profiles of their own, with their genres, tags, BPM range and years, and shows everything about a track with `weave-player track <id>`.
- Never merges two artists, albums or tracks by guess. It asks about look-alikes after an import and merges only what you say.
- Runs offline. One instance can be controlled from another on your own network, and nothing goes through a cloud service.

## Try it

```bash
cargo run -p weave-player -- dev seed      # 60 fake tracks with moods, languages, genres and playlists
cargo run -p weave-player -- search --tag-all lang:spanish,mood:romantic --bpm-max 100
cargo run -p weave-player -- import ~/Music   # your own music; files are only read, never changed
```

## Learn more

- [goals.md](goals.md) has the goals, the full command reference and the design.
- [CONTRIBUTING.md](CONTRIBUTING.md) covers building, running and testing.
- [agents.md](agents.md) lists the invariants every change must respect.
- [docs/adr](docs/adr) records the decisions and why they were made.
- [docs/ui](docs/ui/README.md) is the design of the GUI frontend (iced and iced-libadwaita).
- [credits.md](credits.md) credits the projects Weave learned from.

Weave is pre-0.1: the API and the library file format change without notice, so delete and re-import after an update.
