//! Keyed, hierarchical tags on SQLite.
//!
//! - A tag has an optional key, a value, a case-insensitive identity and at most one parent:
//!   `genre:bachata` and `practice` are equally valid; the colon is syntax, not hierarchy.
//! - Assignments are explicit and flat. The tree only widens queries: a filter for a tag also
//!   reaches its descendants unless it asks for an exact match (`=genre:bachata`).
//! - Any number of kinds of things ([`Owner`]s) can carry tags, all sharing one vocabulary.
//! - [`TagFilter`] (all / any / exclude, keyed clauses, and flat AND blocks of OR alternatives)
//!   rejects nested tag blocks and compiles to parameterized SQL
//!   with [`conditions`], to splice into your own `WHERE`.
//!
//! ```
//! use weaver::{ChildPolicy, Owner, TagFilter, Tags, conditions};
//!
//! const SONG: Owner = Owner {
//!     name: "song", table: "song_tags", col: "song_id",
//!     owner_table: "songs", owner_key: "id", key_type: "TEXT",
//! };
//! static OWNERS: [Owner; 1] = [SONG];
//! let tags = Tags::new(&OWNERS);
//!
//! let mut db = rusqlite::Connection::open_in_memory().unwrap();
//! db.execute_batch("CREATE TABLE songs (id TEXT PRIMARY KEY);").unwrap();
//! db.execute_batch(&tags.schema_sql()).unwrap();
//! db.execute("INSERT INTO songs VALUES ('a')", []).unwrap();
//!
//! tags.create(&mut db, "genre:bachata", None).unwrap();
//! tags.create(&mut db, "genre:sensual", Some("genre:bachata")).unwrap();
//! tags.add(&mut db, &SONG, "a", &["Genre:Sensual".into()]).unwrap();
//!
//! let filter = TagFilter { all: vec!["genre:bachata".into()], ..Default::default() }.normalized();
//! let (sql, args) = conditions(&filter, &SONG, "s.id");
//! let n: i64 = db
//!     .query_row(
//!         &format!("SELECT COUNT(*) FROM songs s WHERE 1=1{sql}"),
//!         rusqlite::params_from_iter(args),
//!         |r| r.get(0),
//!     )
//!     .unwrap();
//! assert_eq!(n, 1);
//! # let _ = ChildPolicy::Detach;
//! ```

pub mod bulk;
mod error;
mod filter;
mod name;
mod sql;
mod store;

pub use error::{Error, Result};
pub use filter::{TagClause, TagFilter, TagOp};
pub use name::{ChildPolicy, EXACT_PREFIX, Tag, TagName, normalize_tag, parse_tag_ref};
pub use sql::conditions;
pub use store::{Owner, Tags};

#[cfg(test)]
mod tests;
