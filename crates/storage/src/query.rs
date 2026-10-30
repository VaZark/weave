//! Translates a `FilterSpec` into parameterized SQL. No user text is interpolated.

use rusqlite::types::Value;
use weave_core::{FilterSpec, normalize_tag};

pub const COLUMNS: &str = "m.track_id, m.title, m.artist, m.album, m.album_artist, m.genre, m.composer, \
                           m.year, m.track_number, m.disc_number, m.duration_ms, m.bpm, \
                           (SELECT slug FROM tracks WHERE id = m.track_id)";

/// Matches the `meta_sort` index, so paging does not need a sort step.
pub const ORDER: &str = " ORDER BY m.artist_norm, m.album_norm, m.title_norm, m.track_id";

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn tag_keys(tags: &[String]) -> Vec<Value> {
    tags.iter().map(|t| Value::Text(normalize_tag(t))).collect()
}

const TAGGED: &str = "SELECT 1 FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
                      WHERE tt.track_id = m.track_id AND g.normalized_name IN";

/// ` FROM … WHERE …` over `track_metadata m`. Expects a normalized, validated filter.
pub fn from_where(f: &FilterSpec) -> (String, Vec<Value>) {
    let mut sql = String::from(" FROM track_metadata m WHERE 1=1");
    let mut args: Vec<Value> = Vec::new();

    if !f.tag_all.is_empty() {
        // Tags are deduplicated by normalize(), so the distinct count must equal the list length.
        sql += &format!(
            " AND (SELECT COUNT(DISTINCT g.normalized_name) FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
               WHERE tt.track_id = m.track_id AND g.normalized_name IN ({})) = ?",
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
    // NULL compares as NULL, so unknown BPM/year never matches a bound.
    for (col, op, v) in [
        ("bpm", ">=", f.bpm_min.map(Value::Real)),
        ("bpm", "<=", f.bpm_max.map(Value::Real)),
        ("year", ">=", f.year_min.map(|y| Value::Integer(y.into()))),
        ("year", "<=", f.year_max.map(|y| Value::Integer(y.into()))),
    ] {
        if let Some(v) = v {
            sql += &format!(" AND m.{col} {op} ?");
            args.push(v);
        }
    }
    for (col, val) in [
        ("title_norm", &f.title),
        ("artist_norm", &f.artist),
        ("album_norm", &f.album),
        ("genre_norm", &f.genre),
    ] {
        if let Some(v) = val {
            sql += &format!(" AND instr(m.{col}, ?) > 0");
            args.push(Value::Text(v.trim().to_lowercase()));
        }
    }
    (sql, args)
}
