# ADR 0005: Instances and remote control, not backends

Status: accepted. The instance identity and the playback switch are implemented; pairing, trust and the network
surface are goals.

## Problem

Weave was designed around capability-based backends (`local`, a future Spotify, third-party plugins). The product is a
local library that can also be controlled from another device on the same network: a phone as a thin client of the
desktop engine, even though the two hold different music. Streaming services and plugins do not fit that, and the
machinery for them (backend ids in slugs, capability descriptors, a backend CLI) only added noise.

## Decision

**Backends are removed**

- There is no `Backend` trait, capability descriptor or backend id. A track has one source, its file path, in
  `sources.path`, apart from its identity. Track slugs are `artist[-album]-title`.
- `crates/local` is plain file code: scanning, lyrics and rodio playback.
- Everything runs locally, or a thin client controls another instance. No cloud accounts, third-party streaming
  services, plugins or library sync between instances.

**An instance is the unit**

- An instance is a full engine with its own library, queue and player. It has a **name**, a slug that defaults to the
  host name and that it will advertise on the network, and **capabilities** chosen by its owner.
- The `playback` capability decides whether it makes sound. Off makes it remote-only: it keeps its library and queue
  and can be controlled, but no host starts, `open_playback` and the embedded `Player` refuse, and the CLI says so.
  A second capability, `serve`, will decide whether it accepts remote clients at all (off by default).
- CLI: `instance show`, `instance name <name>`, `instance playback on|off`. Settings live in a one-row `instance`
  table.
- Instance settings and client management are local-only; no client can reach them.

**Clients are registered and trusted by the owner (goal)**

- Being on the network earns nothing: discovery shows the instance's name only, and every other request needs a
  registered client.
- Registering needs the owner present on the instance (`client pair`): a single-use code that expires in minutes,
  rate-limited. The client presents it with its own name and public key and is stored under a slug.
- Both sides pin each other's keys at pairing, so later connections reach the same instance and are encrypted; every
  request is authenticated by the client's key.
- Trust is per client: `view` (see the queue, session and library) or `control` (also change the queue and playback,
  edit tags and playlists). `client list`, `client trust <name> view|control` and `client revoke <name>` review and
  change it; revoking takes effect at once and drops live connections. The instance logs what each client did.
- The remote surface exposes the `Library` operations and pushes session events; it adds no logic of its own. The
  instance plays, and no library is copied or merged.
- Open, to settle with a spike: discovery (mDNS or a typed address), the transport (TLS with a pinned certificate or a
  Noise handshake), the wire protocol (HTTP and JSON with a WebSocket or server-sent events), and clients when the
  instance sleeps.

## Alternatives

- **Keep backends for Spotify, Jellyfin and plugins:** service terms, credentials and a plugin runtime for a product
  that is now local-first with remote control.
- **Trust anything on the LAN, or one shared password:** a guest on the network could control the instance, and one
  leaked password could not be revoked per device.
- **Sync libraries between instances:** merge and conflict rules for no benefit when a client can use the other
  instance's music directly.
- **A cloud relay for remote access:** accounts, hosting and exposure, against the local-first goal.

## Consequences

- Existing development libraries must be deleted and re-imported; the schema is still the pre-0.1 squashed one.
- A remote-only instance does not yet refuse every playback command: `play` still marks the session as playing, with
  a note on stderr. That changes once clients exist and playback commands can target another instance.
- Seeded and imported tracks both carry plain paths, so a track whose file is gone is unplayable and skipped.
