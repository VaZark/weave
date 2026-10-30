//! Translates a `FilterSpec` into parameterized SQL. No user text is interpolated.

use rusqlite::types::Value;
use weave_core::{FilterSpec, Sort, SortField, normalize_tag};

pub const FROM: &str = " FROM track_metadata m \
     JOIN tracks t ON t.id = m.track_id \
     LEFT JOIN track_annotations a ON a.track_id = m.track_id";

/// Column order is relied on by `Storage::fetch`.
pub const COLUMNS: &str = "m.track_id, m.title, m.artist, m.album, m.album_artist, m.genre, \
     m.composer, m.year, m.track_number, m.disc_number, m.duration_ms, m.bpm, \
     t.slug, COALESCE(a.favorite, 0), a.rating";

/// The default order matches the `meta_sort` index, so paging needs no sort step.
const DEFAULT_ORDER: &str = "m.artist_norm, m.album_norm, m.title_norm, m.track_id";

/// Unknown values sort last in either direction; ties fall back to the default order.
pub fn order_by(sort: Option<Sort>) -> String {
    let Some(Sort { field, descending }) = sort else {
        return format!(" ORDER BY {DEFAULT_ORDER}");
    };
    let expr = match field {
        SortField::Title => "m.title_norm",
        SortField::Artist => "m.artist_norm",
        SortField::Album => "m.album_norm",
        SortField::Genre => "m.genre_norm",
        SortField::Year => "m.year",
        SortField::Bpm => "m.bpm",
        SortField::Duration => "m.duration_ms",
        SortField::Rating => "a.rating",
        SortField::Added => "t.added_at",
    };
    let dir = if descending { "DESC" } else { "ASC" };
    format!(" ORDER BY {expr} IS NULL, {expr} {dir}, {DEFAULT_ORDER}")
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn tag_keys(tags: &[String]) -> Vec<Value> {
    tags.iter().map(|t| Value::Text(normalize_tag(t))).collect()
}

const TAGGED: &str = "SELECT 1 FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
                      WHERE tt.track_id = m.track_id AND g.normalized_name IN";

/// ` WHERE …` for `FROM`. Expects a normalized, validated filter.
pub fn where_clause(f: &FilterSpec) -> (String, Vec<Value>) {
    let mut sql = String::from(" WHERE 1=1");
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
        sql += &format!(
            " AND NOT EXISTS ({TAGGED} ({}))",
            placeholders(f.tag_none.len())
        );
        args.extend(tag_keys(&f.tag_none));
    }
    // NULL compares as NULL, so unknown BPM/year/rating never matches a bound.
    for (col, op, v) in [
        ("m.bpm", ">=", f.bpm_min.map(Value::Real)),
        ("m.bpm", "<=", f.bpm_max.map(Value::Real)),
        ("m.year", ">=", f.year_min.map(|y| Value::Integer(y.into()))),
        ("m.year", "<=", f.year_max.map(|y| Value::Integer(y.into()))),
        (
            "a.rating",
            ">=",
            f.rating_min.map(|r| Value::Integer(r.into())),
        ),
        (
            "a.rating",
            "<=",
            f.rating_max.map(|r| Value::Integer(r.into())),
        ),
    ] {
        if let Some(v) = v {
            sql += &format!(" AND {col} {op} ?");
            args.push(v);
        }
    }
    match f.favorite {
        Some(true) => sql += " AND COALESCE(a.favorite, 0) = 1",
        Some(false) => sql += " AND COALESCE(a.favorite, 0) = 0",
        None => {}
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
