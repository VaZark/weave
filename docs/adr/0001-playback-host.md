# ADR 0001: Playback host, audio stack, MPRIS and lyrics

Status: accepted for 0.a4. Reversible: the engine API is the contract, so each piece can be swapped.

## Problem

0.a3 made the queue and session persistent, but nothing plays. 0.a4 needs audio output, desktop media-key control
(MPRIS) and synchronized lyrics. Short-lived commands cannot hold audio, an MPRIS name or timers, so something has to
keep running. The original "no mandatory background services" non-goal is dropped: every music player has a resident
part, and users expect `play` to make sound.

## Decision

- **A resident host process** (`weave-player host`) plays audio, serves MPRIS and enforces timers. All other commands
  keep editing the persisted session; the host follows it. Commands that leave the session playing start the host if
  none is running (detached, logging to `<db>.host.log`); `weave-player quit` stops it and `host` runs it in the
  foreground. `--no-host` (or `WEAVE_NO_HOST=1`) disables the auto-start, and the library and queue still work with no
  host at all. `watch` is replaced by `host`.
- **The host and the commands talk through the database.** The engine publishes a track-start counter, a seek counter,
  the volume and a position anchor in the player state; the host polls (every 100 ms) and publishes its real position
  back about once a second.
- **Audio: rodio** (symphonia decoders, cpal output) behind a small `Playback` trait in core, implemented by the
  `local` backend behind an `audio` cargo feature, so the engine and tests never need a sound device.
- **MPRIS: mpris-server** (zbus, no tokio) on its own thread, bridged to the host by channels. MPRIS calls go through
  the same engine methods the CLI uses.
- **Lyrics:** LRC parsing and the current-line lookup live in core. The `local` backend reads a `.lrc` file next to the
  audio file, then falls back to embedded lyrics (synced if they contain LRC timestamps).
- **One host per library:** an exclusive lock on `<db>.host.lock`; a second host exits quietly, which also makes
  concurrent auto-starts safe.

## Alternatives

- **Blocking `play` (mpv style):** one process per play and no shared session; control only through MPRIS. Rejected:
  it throws away the persisted session.
- **Blocking `play` only, no resident process:** rejected, see above.
- **IPC socket between CLI and host:** lower latency than polling, but a protocol to design and version. Polling a
  local SQLite file every 100 ms is cheap; revisit if latency matters.
- **symphonia + cpal directly:** more control (gapless, resampling) for roughly 500 more lines. rodio can be replaced
  behind the `Playback` trait later.
- **souvlaki:** simpler, cross-platform media controls, but less of MPRIS than mpris-server. Linux is the target.

## Consequences

- Audio needs the ALSA/PulseAudio development files at build time (`audio` feature, on by default for the binary).
- Gapless playback is not attempted: each track is started when the previous one ends.
- A command races the host on the session in the same way two commands do (documented in goals.md).

## Notes from building it

- rodio cannot seek in roughly the first 100 ms of a track (`RandomAccessNotSupported`). The `local` backend therefore
  restarts the file for early seeks and ignores a start offset below 250 ms.
- Requesting a bus name another player already owns does not fail in zbus; it is queued. The host looks for an owner
  first and refuses with a clear message.
- The host is tested against a fake `Playback`; the audio and MPRIS integration tests skip themselves when there is no
  output device or session bus.
