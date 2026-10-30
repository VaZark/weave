# ADR 0003: OS media controls through souvlaki (1.0 branch)

Status: accepted for 0.a5. Reversible: the media code is one module behind a small `Remote`/`Snapshot` interface.

## Problem

0.a4 registered with the desktop through `mpris-server`, which is Linux only. Weave is to be embedded in other
players, which may run on Windows and macOS, so the media-controls layer should be cross-platform.

## Decision

- **souvlaki, from its in-progress `1.0` branch** (a git dependency, locked by `Cargo.lock`), replaces
  `mpris-server`. Its 1.0 API adds what the old stable release lacked: shuffle, repeat, volume, seek requests and
  permissions.
- **Its pure-Rust `mpris_zbus` backend** on Linux, so no libdbus is needed. `zbus` is also a direct dependency, only
  to look for a bus name another player already owns (zbus queues behind an owner instead of failing).
- **Linux only for now.** On other platforms the player reports "not wired yet" and carries on without media
  controls. The rest (events, metadata, playback state) is souvlaki's cross-platform surface, so wiring Windows and
  macOS means building their platform configs.
- **Platform-neutral names:** the feature is `media-controls` and the options are `PlayerOptions::media_controls` and
  `media_controls_name`, replacing the MPRIS-specific ones.
- **The interface stays ours.** Souvlaki events become `Remote` requests and session snapshots become souvlaki
  calls, in pure, tested functions; the player never touches souvlaki types.

## Alternatives

- **Keep `mpris-server`** and add per-platform crates later: more code per platform, no shared model.
- **souvlaki 0.9.x (released):** no shuffle, repeat or volume requests, and a stringly permissions model.
- **souvlaki with its libdbus backend:** needs libdbus development files; the zbus one does not.

## Consequences

- **A git dependency means the library cannot be published to crates.io** until souvlaki 1.0 is released. That is
  fine before 0.1; switch to the release then.
- **Track id:** souvlaki 1.0 sends a constant `mpris:trackid` (`/`), so we no longer send one per track.
- **`Seeked`:** every `set_playback` emits an MPRIS `Seeked` signal, so the position is pushed on a change and about
  once a second while playing, not twice a second.
- **Noise:** souvlaki prints (`println!`) if its service thread dies; that goes to the host's log or the embedder's
  stdout.
- The branch is in progress, so an update may change its API.
