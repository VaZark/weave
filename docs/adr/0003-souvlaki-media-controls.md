# ADR 0003: OS media controls through souvlaki (1.0 branch)

Status: accepted. Reversible: the media code is one module behind a small `Remote`/`Snapshot` interface.

## Problem

The player should appear in the desktop's media controls (media keys, now-playing widgets, `playerctl`) and be usable from other players on Windows and macOS, so the layer should be cross-platform.

## Decision

- **souvlaki, from its in-progress `1.0` branch** (a git dependency, locked by `Cargo.lock`). Its 1.0 API adds what the stable release lacks: shuffle, repeat, volume, seek requests and permissions.
- **Its pure-Rust `mpris_zbus` backend** on Linux, so no libdbus is needed. `zbus` is also a direct dependency, only to look for a bus name another player already owns (zbus queues behind an owner instead of failing).
- **Linux (MPRIS) only for now.** Elsewhere the player reports "not wired yet" and carries on without media controls. Wiring Windows and macOS means building their platform configs.
- **Platform-neutral names:** the feature is `media-controls`; the options are `PlayerOptions::media_controls` and `media_controls_name`.
- **The interface stays ours.** Souvlaki events become `Remote` requests and session snapshots become souvlaki calls, in pure, tested functions; the player never touches souvlaki types.

## Alternatives

- **`mpris-server`:** Linux only, and more code per platform with no shared model.
- **souvlaki 0.9.x (released):** no shuffle, repeat or volume requests, and a stringly permissions model.
- **souvlaki with its libdbus backend:** needs libdbus development files; the zbus one does not.

## Consequences

- **A git dependency means the library cannot be published to crates.io** until souvlaki 1.0 is released. That is fine before 0.1; switch to the release then.
- **Track id:** souvlaki 1.0 sends a constant `mpris:trackid` (`/`), not one per track.
- **`Seeked`:** every `set_playback` emits an MPRIS `Seeked` signal, so the position is pushed on a change and about once a second while playing.
- **Noise:** souvlaki prints (`println!`) if its service thread dies; that goes to the host's log or the embedder's stdout.
- The branch is in progress, so an update may change its API.
