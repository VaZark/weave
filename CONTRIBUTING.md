# Contributing to Weave

Read [goals.md](goals.md) for scope and CLI semantics and [agents.md](agents.md) for the invariants and
conventions every change must respect. This page covers getting a working checkout.

## Prerequisites

- **Rust 1.85 or newer** (edition 2024). Install with [rustup](https://rustup.rs).
- **SQLite 3.14.0 or newer** as a system library (`sqlite` on Arch, `libsqlite3-dev` on Debian/Ubuntu), unless you
  build with `sqlite-from-source` (see [Build rules](#build-rules)).
- **A C toolchain** (`gcc` or `clang`): Rust links binaries with it on Linux.
- Git. No database server, daemon or account is required.

The GPUI desktop app is not built yet (planned for 0.a3); only the CLI and the engine exist today.

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

- Default: links the system SQLite (3.14.0 or newer, checked at startup).
- `--features sqlite-from-source`: compiles SQLite and links it statically. Set it on the binary, e.g.
  `cargo build -p weave-player --features sqlite-from-source`.
- `weave-player dev ...` exists only in debug builds.

## Run and test

```bash
cargo test --workspace        # all unit and integration tests
cargo clippy --workspace --all-targets
cargo fmt --all
```

Use a throwaway library while developing so your real one is never touched:

```bash
export WEAVE_DB=/tmp/weave-dev.db     # or pass --db <path> to any command
```

The default library lives at `$XDG_DATA_HOME/weave/library.db` (`~/.local/share/weave/library.db`).
Until 0.1 the schema is a single squashed migration, so an older database is rejected with an
"incompatible database schema" error. Delete the file (and its `-wal`/`-shm` files) and re-import.

## Populate a database with test data

Debug builds include a developer command that fills the library with fake tracks, tags and playlists. No audio
files are read, and release builds do not contain it.

```bash
weave-player dev seed                # 60 tracks, tags, and 3 smart playlists
weave-player dev seed --count 100    # up to 100
```

It is idempotent: running it again adds nothing that already exists. Seeded tracks use the backend id `seed`
(their IDs end in `-seed`) and have no files, so they are not playable. Then try:

```bash
weave-player list
weave-player search --tag-all bachata,90s --bpm-min 90
weave-player playlist list
weave-player playlist show "Bachata 90s"
```

To import real music instead (files are only read, never modified):

```bash
weave-player import ~/Music
```

## Project layout

```text
crates/core/       types, FilterSpec, track slugs, backend contracts
crates/storage/    SQLite schema, migrations, query building
crates/engine/     the single API frontends call
backends/local/    file scanning (audio detected by content, not extension)
apps/cli/          the `weave-player` binary (apps/cli/src/dev.rs is debug-only)
```

Frontends call the engine and never touch the database directly, and filtering or playlist rules live in the
engine, never only in a frontend.

## Making a change

- Add or update tests next to the code. Storage tests use an in-memory database (`Storage::open_in_memory`).
- A feature is done when it is tested, exposed through the engine, documented with an example in
  [goals.md](goals.md), and consistent across interfaces (see the definition of done in [agents.md](agents.md)).
- Architecture decisions need a short ADR first (problem, alternatives, tradeoffs, reversibility).
- Do not reuse upstream code or assets without checking the license and updating [credits.md](credits.md).
