# weaver

Keyed, hierarchical tags on SQLite, vendored from [Weave](../../README.md) so other projects can use it.

- Names: optional key plus value (`genre:bachata`, `practice`), case-insensitive identity.
- A tag has at most one parent. Assignments are flat; filters reach descendants unless exact (`=genre:bachata`).
- Any number of tagged kinds (`Owner`) share one vocabulary and tree.
- `TagFilter` (all / any / exclude, keyed clauses) compiles to parameterized SQL with `conditions`.
- Cycle checks, delete policies (`Detach` / `Delete`) and merges (`copy`) run in transactions.

> **Note:** `weaver` is vendored inside the Weave Player repo for now and should eventually move to its own repository. Until then, keep it free of any Weave dependency (see `agents.md`), so the move is just a copy and a version tag.

## Use it elsewhere

Copy this directory into your project (or keep it as a git subtree) and depend on it by path:

```toml
weaver = { path = "vendor/weaver" }            # system SQLite
weaver = { path = "vendor/weaver", features = ["bundled"] }
```

It depends only on `rusqlite`, `serde` and `thiserror`, and nothing from Weave. See the example in `src/lib.rs`
and the tests in `src/tests.rs`. Weave's own binding is `crates/storage/src/tags.rs`.

Run your schema setup with `Tags::schema_sql()` after creating the owner tables, and enable `PRAGMA foreign_keys = ON`
so deleting an owner removes its assignments.
