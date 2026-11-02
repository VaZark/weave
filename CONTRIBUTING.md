# Contributing to Weave

Read [goals.md](goals.md) for scope and CLI semantics and [agents.md](agents.md) for the invariants and conventions every change must respect. This page covers getting a working checkout.

## Prerequisites

- **Rust 1.85 or newer** (edition 2024). Install with [rustup](https://rustup.rs).
- **SQLite 3.14.0 or newer** as a system library (`sqlite` on Arch, `libsqlite3-dev` on Debian/Ubuntu), unless you build with `sqlite-from-source` (see [Build rules](#build-rules)).
- **A C toolchain** (`gcc` or `clang`): Rust links binaries with it on Linux.
- **ALSA development files** for audio output (`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu), unless you build without the `audio` feature.
- Git. No database server or account is required.

Weave is a library (`weave`) that other players embed, plus the `weave-player` CLI built on it. GUIs are separate projects.

## Get the code and build

```bash
git clone <repository-url> weave
cd weave
cargo build            # debug build: target/debug/weave-player
cargo build --release  # optimized build: target/release/weave-player
cargo install --path apps/cli   # install weave-player into ~/.cargo/bin
```

Without installing, run it with `cargo run -p weave-player -- <args>`.

## Build rules

Two artifacts, built separately:

- The library: `cargo build -p weave` (add `--features audio,media-controls`). It has no CLI code or dependencies. Try the embedding API with `cargo run -p weave --example embedded_player --features audio,media-controls -- ~/Music`.
- The CLI: `cargo build -p weave-player`. It depends on `weave` only.

- Default: links the system SQLite (3.14.0 or newer, checked at startup).
- `--features sqlite-from-source`: compiles SQLite and links it statically. Set it on the binary, e.g. `cargo build -p weave-player --features sqlite-from-source`.
- The library has no default features; its features are `audio` (rodio, needs ALSA headers), `media-controls` (OS media keys and widgets through souvlaki's 1.0 branch, Linux only for now) and `sqlite-from-source`. The CLI turns `audio` and `media-controls` on by default; build it with `--no-default-features` for a binary that cannot play sound or register with the desktop.
- `weave-player dev ...` exists only in debug builds.

## Playback while developing

Commands that start playback launch the playback host (`weave-player host`) in the background; its log is `<library>.host.log` next to the database, `weave-player now` shows whether it runs and `weave-player quit` stops it. Use `--no-host` (or `WEAVE_NO_HOST=1`) to edit the session without ever starting one, and `weave-player host` to run it in the foreground. The audio test plays silence and the media-controls tests talk to the session bus; both skip themselves where there is no output device or no bus.

## Run and test

```bash
cargo test --workspace        # all unit and integration tests
cargo clippy --workspace --all-targets
cargo fmt --all
```

Markdown is formatted with [dprint's markdown plugin](https://dprint.dev/plugins/markdown/) (`dprint fmt`, configured in `dprint.json`). Install dprint however you like ([install page](https://dprint.dev/install/)); it must be on your `PATH`. A pre-commit hook formats staged markdown and re-stages it; enable it once per clone with `git config core.hooksPath .githooks`.

Use a throwaway library while developing so your real one is never touched:

```bash
export WEAVE_DB=/tmp/weave-dev.db     # or pass --db <path> to any command
```

The default library lives at `$XDG_DATA_HOME/weave/library.db` (`~/.local/share/weave/library.db`). Until 0.1 the schema is a single squashed migration, so an older database is rejected with an "incompatible database schema" error. Delete the file (and its `-wal`/`-shm` files) and re-import.

## Populate a database with test data

Debug builds include a developer command that fills the library with fake tracks, tags and playlists. No audio files are read, and release builds do not contain it.

```bash
weave-player dev seed                # 60 tracks with a tag tree, ratings and 9 playlists
weave-player dev seed --count 100    # up to 100
```

It is idempotent: running it again adds nothing that already exists. Seeded tracks have no files behind them, so they are not playable. Then try:

```bash
weave-player list --show-ids    # --show-ids adds the slug ID column
weave-player search --tag-all lang:spanish,mood:romantic --bpm-max 100   # mood, language and beat
weave-player search --tag-all lang:romance --bpm-min 90        # a parent tag matches its children
weave-player search --tag-all genre:bachata-sensual              # exact: that tag only
weave-player tag list                                          # the seeded tree: genre:latin > bachata > ...
weave-player artist list --tag-any genre=latin                 # artists carry tags too
weave-player artist aventura                                   # a profile: albums, genres, ratings ...
weave-player dedup list                                        # Daft Punk / Daft-Punk, and two copies of a track: merge or separate
weave-player album list --artist aventura
weave-player track <track-id>                                  # everything about one track
weave-player playlist list
weave-player playlist show "Bachata 90s"
weave-player play playlist road-trip --shuffle   # fill the queue (seeded tracks have no files, so nothing plays)
weave-player play random --tag-any genre=bachata   # a random order of what matches
weave-player queue show
weave-player now
```

In a debug build, `weave-player dev finish-track` simulates the current track ending by itself, which is what the playback host will report; use it to try repeat, shuffle and `queue end continue`.

To import real music instead (files are only read, never modified):

```bash
weave-player import ~/Music
```

## Project layout

```text
crates/weave/      the library to embed: re-exports the crates below and runs the embedded Player
crates/core/       types, FilterSpec, track slugs, instance identity
crates/storage/    SQLite schema, migrations, query building
crates/engine/     the single API frontends call
crates/local/     file scanning, lyrics and playback (audio detected by content, not extension)
apps/cli/          the `weave-player` binary, a frontend over `weave` only (src/dev.rs is debug-only)
```

Frontends call the library and never touch the database directly, and filtering, playback or playlist rules live in the library, never only in a frontend.

## Making a change

- Add or update tests next to the code. Storage tests use an in-memory database (`Storage::open_in_memory`).
- A feature is done when it is tested, exposed through the engine, documented with an example in [goals.md](goals.md), and consistent across interfaces (see the definition of done in [agents.md](agents.md)).
- Architecture decisions need a short ADR first (problem, alternatives, tradeoffs, reversibility).
- Do not reuse upstream code or assets without checking the license and updating [credits.md](credits.md).
