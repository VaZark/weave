# ADR 0001: Playback host, audio stack and lyrics

Status: accepted. Reversible: the engine API is the contract, so each piece can be swapped.

## Problem

The queue and session are persistent, but short-lived commands cannot hold audio, OS media controls or timers, so something has to keep running. The original "no mandatory background services" non-goal is dropped: every music player has a resident part, and users expect `play` to make sound.

## Decision

- **A resident host process** (`weave-player host`) plays audio, registers with the OS media controls and enforces timers. All other commands keep editing the persisted session; the host follows it. Commands that leave the session playing start the host if none is running (detached, logging to `<db>.host.log`); `weave-player quit` stops it and `host` runs it in the foreground. `--no-host` (or `WEAVE_NO_HOST=1`) disables the auto-start, and the library and queue still work with no host at all.
- **The host and the commands talk through the database.** The engine publishes a track-start counter, a seek counter, the volume and a position anchor in the player state; the host polls every 100 ms and publishes its real position back about once a second.
- **Audio: rodio** (symphonia decoders, cpal output) behind a small `Playback` trait in core that takes a file path. `crates/local` implements it behind an `audio` cargo feature, so the engine and tests never need a sound device.
- **Lyrics:** LRC parsing and the current-line lookup live in core. `crates/local` reads a `.lrc` file next to the audio file, then falls back to lyrics embedded in its tags (synchronized if they contain LRC timestamps).
- **One host per library:** an exclusive lock on `<db>.host.lock`; a second host exits quietly, which also makes concurrent auto-starts safe.
- **Unplayable tracks** (a missing or undecodable file) are skipped with an error event, and a queue with nothing playable stops instead of looping.
- **A remote-only instance never plays** (see [ADR 0005](0005-instances-and-remote-control.md)): no host is started, and `open_playback` and the embedded `Player` refuse.

## Alternatives

- **Blocking `play` (mpv style):** one process per play and no shared session. Rejected: it throws away the persisted session.
- **IPC socket between CLI and host:** lower latency than polling, but a protocol to design and version. Polling a local SQLite file every 100 ms is cheap; revisit if latency matters. A network surface is a separate decision (ADR 0005).
- **symphonia + cpal directly:** more control (gapless, resampling) for roughly 500 more lines. rodio can be replaced behind the `Playback` trait later.

## Consequences

- Audio needs the ALSA development files at build time on Linux (`audio` feature, on by default for the binary).
- Gapless playback is not attempted: each track is started when the previous one ends.
- A command races the host on the session the way two commands do (documented in `goals.md`).
- rodio cannot seek in roughly the first 100 ms of a track, so local playback restarts the file for early seeks and ignores a start offset below 250 ms.
- The host is tested against a fake `Playback`; audio integration tests skip themselves when there is no output device.
