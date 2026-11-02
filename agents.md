# Weave: Agent and Contributor Instructions

This file guides coding agents and human contributors working on Weave. Read `goals.md` before changing architecture or feature and filter semantics; see `credits.md` for attribution.

## Mission

Build an open-source, local-first, source-neutral music management engine used by a CLI and, as an embeddable library, by other players (GUI frontends are separate projects). Treat arbitrary user tags and continuously evaluated smart playlists as core features, not optional enhancements.

## Non-negotiable invariants

1. **One canonical library:** A track's Weave identity must not be its file path, Spotify URI or any backend-specific ID.
2. **Slug IDs:** Every ID users, scripts, frontends or plugins see is a slug (lowercase, readable, assigned once, unique). Internal surrogate keys never leak. New entities get a slug-based ID from the start; see Identifiers in `goals.md`.
3. **User annotations are protected:** Enrichment and backend sync must not overwrite local tags, ratings or metadata overrides without an explicit user action.
4. **Backends are capability-driven:** Do not hardcode Spotify or `local` special cases into the query, metadata or playlist domain model. The local backend is bundled, but follows shared contracts.
5. **One engine API:** Never implement filtering, persistence rules, playback or playlist semantics only inside a frontend. The CLI depends on the `weave` library crate and nothing else in the workspace; whatever a second player would need lives in the library.
6. **Dynamic playlists are queries:** Saved dynamic (smart) playlists retain filter definitions, with their sort and limit, and refresh against current canonical library state. Manual playlists are explicit ordered lists; never mix the two models.
7. **Safe imports:** Read-only imports by default; no moves, renames, tag rewrites or destructive operations without explicit opt-in.
8. **Backend boundaries:** Plugins cannot directly mutate the canonical SQLite database. All changes pass engine validation, provenance and merge policies.
9. **Compliant integration:** Verify current service terms and capabilities before implementing remote playback, lyrics overlays, content mixing or playlist export.
10. **Tags are for descriptive metadata and user-defined tags only:** Identifying data (title, artist, album, and source or path identities) is never a tag; it stays in structured columns and is filtered by dedicated text predicates. Playback state and user-protected annotations (favorite, rating) are not tags either. Do not map a new field to a tag unless it describes the track rather than identifies it.
11. **Keep it lean:** Favor working vertical slices and tests over speculative abstractions or premature crate extraction.

## Recommended implementation sequence

1. Define domain types and SQLite migrations, plus correctness tests for source mappings and tags.
2. Implement bundled local backend and read-only importing.
3. Implement `FilterSpec` and query evaluation; cover all/any/none, empty groups, Unicode tags, missing BPM, and inclusive boundaries.
4. Add CLI commands with stable, scriptable output and meaningful exit codes.
5. Save smart playlist definitions and test live membership updates on edit/import.
6. Keep the `weave` library and its embedded player sufficient for any frontend; GUIs are separate projects that embed it.
7. Provide local playback and queue control before optional lyrics; then validate a second backend implementation.
8. Build installable plugin runtime only after the backend contract is exercised by multiple implementations.

## Coding conventions

- Language: Rust for the library and the CLI; use `clap` for CLI parsing and SQLite for storage (system library by default, minimum 3.14.0; the opt-in `sqlite-from-source` feature compiles and statically links it).
- Respect the specs defined in `goals.md`
- Keep the domain model independent of any UI, SQL details and backend SDKs.
- Prefer small explicit types, typed errors and transparent operations over magic configuration.
- Use transactions for multi-table mutations, migrations for schema changes and tests for data integrity.
- Use fixtures and mocks for remote backends; integration tests must not require real Spotify credentials.
- Document new capability contracts and version compatibility; model `supported`, `authorized`, and `available` separately.
- Keep the library and queue usable with no playback host running; avoid a hosted account or cloud database for the MVP.
- Use clear terminology: `backend`, `capability`, `recording`, `source reference`, `tag`, `filter`, `smart playlist` (dynamic), `manual playlist`, `queue` and `session`.

## Definition of done

A feature is complete only when its engine behavior is tested, available through the relevant application API, documented with an example, and behaves consistently in all implemented interfaces. Backend-dependent failures should be reported without corrupting canonical library state.

## Decision process

- For architecture decisions, state the problem, alternatives, tradeoffs and reversibility in a brief ADR before implementation.
- Prefer proving difficult assumptions with small spikes (e.g. source-neutral identity; plugin lifecycle; concurrent DB access).
- Do not silently broaden scope, rewrite existing working systems or invent provider capabilities.
- If documentation and code disagree, report the mismatch and propose an explicit correction.

## Credits and attribution

See `credits.md`. Verify the applicable license and preserve attribution before reusing any upstream source or assets.
