# ADR 0002: A library artifact with an embedded player, and a CLI that uses it

Status: accepted for 0.a5. Reversible: the facade crate only re-exports and hosts the player loop.

## Problem

Weave is meant to be the backend logic of other players (GUI frontends are separate projects), but the
useful pieces were split across four internal crates and the playback host lived inside the CLI binary. An
embedding app would have had to depend on four crates and could not play audio without launching the CLI.

## Decision

- **Two artifacts from one workspace.** The `weave` library crate (`cargo build -p weave`) and the
  `weave-player` CLI (`cargo build -p weave-player`). The library build contains no CLI code or dependencies.
- **`weave` is a facade.** It re-exports `Library` and the domain types (`weave::core`) and owns the embedded
  `Player`. The internal crates (`core`, `engine`, `storage`, `local`) stay separate implementation details.
- **The player moves into the library.** Following the session, driving audio, publishing the position, the sleep
  timer, MPRIS and the one-player-per-library lock now live in `weave`. A `Player` is a thread with its own database
  connection, started with `Player::start(db, options)`, reporting typed `PlayerEvent`s on a channel.
- **Embedding works like the CLI host:** the app opens a `Library` and edits the session; the `Player` follows it
  through the database. No new IPC and no new concepts. The embedder can supply its own audio output (`Audio::Custom`).
- **The CLI depends on `weave` only.** Its `host` command runs the library's `Player`; what stays in the CLI is
  starting a detached `weave-player host` process and printing.
- **No default features** on the library, like the other crates: embedders choose `audio`, `mpris`,
  `sqlite-from-source`.

## Alternatives

- **One merged crate:** simpler to depend on, but throws away the layering that keeps the domain independent of SQL
  and backends (`agents.md`). The facade gives the same single dependency.
- **A C ABI (`cdylib` + header) now:** would serve non-Rust players, but is a large design (ownership, errors,
  callbacks) with nothing to validate it yet. The Rust API comes first; a C layer can wrap it later.
- **Keep the host in the CLI:** an embedding app would have to spawn the CLI or duplicate the player.
- **A socket or in-process channel API instead of the database:** more machinery than polling a local SQLite file
  every 100 ms, which the CLI already relies on; revisit if latency matters.

## Consequences

- An embedding app and the CLI's host exclude each other through the lock, which is the behavior we want: CLI
  commands still work against the same library, and the app's player does the playing.
- A player started late resumes a track in place only if it is already a second in; a track noticed a tick late
  starts from its beginning.
- The API is unstable until 0.1, and Rust only.
