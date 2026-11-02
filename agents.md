# Weave: Agent and Contributor Instructions

This file guides coding agents and human contributors working on Weave. Read `goals.md` before changing architecture or feature and filter semantics; see `credits.md` for attribution.

## Mission

Build an open-source, local-first music management engine used by a CLI and, as an embeddable library, by other players (GUI frontends are separate projects). One instance can be controlled from another (a phone as a thin client of the desktop engine) over the user's own network. Treat arbitrary user tags and continuously evaluated smart playlists as core features, not optional enhancements.

## Non-negotiable invariants

1. **One canonical library:** A track's Weave identity must not be its file path or any other external ID.
2. **Slug IDs:** Every ID users, scripts, frontends or remote clients see is a slug (lowercase, readable, assigned once, unique). Internal surrogate keys never leak. New entities get a slug-based ID from the start; see Identifiers in `goals.md`.
3. **User annotations are protected:** Enrichment and imports must not overwrite local tags, ratings or metadata overrides without an explicit user action.
4. **Instances are independent:** Each instance owns its library. Never sync or merge libraries between instances; remote control acts on the library of the instance being controlled, and slugs resolve there.
5. **One engine API:** Never implement filtering, persistence rules, playback or playlist semantics only inside a frontend. The CLI depends on the `weave` library crate and nothing else in the workspace; whatever a second player would need lives in the library.
6. **Dynamic playlists are queries:** Saved dynamic (smart) playlists retain filter definitions, with their sort and limit, and refresh against current canonical library state. Manual playlists are explicit ordered lists; never mix the two models.
7. **Safe imports:** Read-only imports by default; no moves, renames, tag rewrites or destructive operations without explicit opt-in.
8. **Remote boundaries:** A remote client never touches the database. Everything goes through the owning instance's engine API and its validation. Remote access is opt-in, off by default and limited to the local network. A client is trusted only after the owner registers it on the instance; trust is per client (`view` or `control`), authenticated on every request and revocable at once. Instance settings (name, capabilities) and client management are local-only.
9. **Local or thin client, nothing else:** No cloud accounts, hosted services, third-party streaming integrations or plugin runtimes. Network use is only between the user's own instances.
10. **Tags are for the genre and user-defined tags only:** Identifying data (title, artist, album, and source or path identities) is never a tag; it stays in structured columns and is filtered by dedicated text predicates. Playback state and user-protected annotations (rating) are not tags either. Do not map a new field to a tag unless it describes the track rather than identifies it. Tags can go on tracks, artists, albums and playlists alike.
11. **Never merge by guess:** Import makes a new artist rather than attach to one already in the library, because the same name can be different people. Artists, albums and tracks that look alike are suggested and asked about, and merged only when the user says so. Duplicate checking is one mechanism for all three kinds.
12. **Keep it lean:** Favor working vertical slices and tests over speculative abstractions or premature crate extraction.

## Recommended implementation sequence

1. Define domain types and SQLite migrations, plus correctness tests for source mappings and tags.
2. Implement local file importing (read-only) and playback.
3. Implement `FilterSpec` and query evaluation; cover all/any/none, empty groups, Unicode tags, missing BPM, and inclusive boundaries.
4. Add CLI commands with stable, scriptable output and meaningful exit codes.
5. Save smart playlist definitions and test live membership updates on edit/import.
6. Keep the `weave` library and its embedded player sufficient for any frontend; GUIs are separate projects that embed it.
7. Provide local playback and queue control before optional lyrics; then the remote-control protocol and a thin client.
8. Spike discovery, pairing and transport on a LAN before committing to a wire protocol.

## Coding conventions

- Language: Rust for the library and the CLI; use `clap` for CLI parsing and SQLite for storage (system library by default, minimum 3.14.0; the opt-in `sqlite-from-source` feature compiles and statically links it).
- Respect the specs defined in `goals.md`
- Keep the domain model independent of any UI, SQL details and audio SDKs.
- Prefer small explicit types, typed errors and transparent operations over magic configuration.
- Use transactions for multi-table mutations, migrations for schema changes and tests for data integrity.
- Test the remote surface against in-process instances; tests must not need a second device or a real network.
- Document new capability contracts and version compatibility; model `supported`, `authorized`, and `available` separately.
- Keep the library and queue usable with no playback host running; avoid a hosted account or cloud database for the MVP.
- Use clear terminology: `instance`, `thin client`, `source`, `tag`, `filter`, `smart playlist` (dynamic), `manual playlist`, `queue` and `session`.

## Definition of done

A feature is complete only when its engine behavior is tested, available through the relevant application API, documented with an example, and behaves consistently in all implemented interfaces. Network and remote failures should be reported without corrupting canonical library state.

## Decision process

- For architecture decisions, state the problem, alternatives, tradeoffs and reversibility in a brief ADR before implementation.
- Prefer proving difficult assumptions with small spikes (e.g. path-independent identity; remote discovery and pairing; concurrent DB access).
- Do not silently broaden scope, rewrite existing working systems or invent provider capabilities.
- If documentation and code disagree, report the mismatch and propose an explicit correction.

## Credits and attribution

See `credits.md`. Verify the applicable license and preserve attribution before reusing any upstream source or assets.
