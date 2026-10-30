//! Translates a `FilterSpec` into parameterized SQL. No user text is interpolated.

use rusqlite::{Connection, functions::FunctionFlags, types::Value};
use weave_core::{Error, FilterSpec, Result, normalize_tag};

/// SQLite's built-in `lower()` is ASCII-only; register a Unicode-aware one.
pub fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "ulower",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(ctx.get::<Option<String>>(0)?.map(|s| s.to_lowercase())),
    )
    .map_err(|e| Error::Storage(e.to_string()))
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn tag_keys(tags: &[String]) -> Vec<Value> {
    tags.iter().map(|t| Value::Text(normalize_tag(t))).collect()
}

const TAGGED: &str = "SELECT 1 FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
                      WHERE tt.track_id = t.id AND g.normalized_name IN";

/// Expects an already-normalized, validated filter.
pub fn build(f: &FilterSpec) -> (String, Vec<Value>) {
    let mut sql = String::from("SELECT t.id, t.title, t.artist, t.album, t.bpm FROM tracks t WHERE 1=1");
    let mut args: Vec<Value> = Vec::new();

    if !f.tag_all.is_empty() {
        // Tags are deduplicated by normalize(), so the distinct count must equal the list length.
        sql += &format!(
            " AND (SELECT COUNT(DISTINCT g.normalized_name) FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
               WHERE tt.track_id = t.id AND g.normalized_name IN ({})) = ?",
            placeholders(f.tag_all.len())
        );
        args.extend(tag_keys(&f.tag_all));
        args.push(Value::Integer(f.tag_all.len() as i64));
    }
    if !f.tag_any.is_empty() {
        sql += &format!(" AND EXISTS ({TAGGED} ({}))", placeholders(f.tag_any.len()));
        args.extend(tag_keys(&f.tag_any));
    }
    if !f.tag_none.is_empty() {
        sql += &format!(" AND NOT EXISTS ({TAGGED} ({}))", placeholders(f.tag_none.len()));
        args.extend(tag_keys(&f.tag_none));
    }
    // NULL bpm compares as NULL, so unknown BPM never matches a bound.
    if let Some(min) = f.bpm_min {
        sql += " AND t.bpm >= ?";
        args.push(Value::Real(min));
    }
    if let Some(max) = f.bpm_max {
        sql += " AND t.bpm <= ?";
        args.push(Value::Real(max));
    }
    for (col, val) in [("title", &f.title), ("artist", &f.artist), ("album", &f.album)] {
        if let Some(v) = val {
            sql += &format!(" AND instr(ulower(t.{col}), ?) > 0");
            args.push(Value::Text(v.trim().to_lowercase()));
        }
    }
    sql += " ORDER BY ulower(t.artist), ulower(t.album), ulower(t.title), t.id";
    (sql, args)
}
