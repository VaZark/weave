//! Translates a `FilterSpec` into parameterized SQL. No user text is interpolated.

use rusqlite::types::Value;
use weave_player_core::{FilterSpec, Sort, SortField};
use weaver::Owner;

use crate::tags::{ALBUM, ARTIST, TRACK};

pub const FROM: &str = " FROM track_metadata m \
     JOIN tracks t ON t.id = m.track_id \
     LEFT JOIN track_annotations a ON a.track_id = m.track_id";

/// Column order is relied on by `Storage::fetch`.
pub const COLUMNS: &str = "m.track_id, m.title, m.artist, m.album, m.album_artist, m.genre, \
     m.composer, m.year, m.track_number, m.disc_number, m.duration_ms, m.bpm, \
     t.slug, a.rating";

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

/// Where tag assignments live for one kind of thing, and how the query reaches its key.
#[derive(Clone, Copy)]
pub struct TagOwner {
    pub owner: &'static Owner,
    /// The owner's key as the surrounding query spells it.
    pub outer: &'static str,
}

pub const TRACK_TAGS: TagOwner = TagOwner {
    owner: &TRACK,
    outer: "m.track_id",
};
pub const ARTIST_TAGS: TagOwner = TagOwner {
    owner: &ARTIST,
    outer: "ar.id",
};
pub const ALBUM_TAGS: TagOwner = TagOwner {
    owner: &ALBUM,
    outer: "al.id",
};

/// The tag conditions of a filter (flat lists and clauses) as ` AND …` terms over
/// the given kind of owner. Expects a normalized, validated filter.
pub fn tag_conditions(f: &FilterSpec, o: TagOwner) -> (String, Vec<Value>) {
    weaver::conditions(&f.tag_filter(), o.owner, o.outer)
}

/// The filter's own conditions as ` AND …` terms for `FROM` (the caller supplies ` WHERE 1=1`).
/// Expects a normalized, validated filter; its playlist sources are added by the storage layer.
pub fn conditions(f: &FilterSpec) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let (tag_sql, mut args) = tag_conditions(f, TRACK_TAGS);
    sql += &tag_sql;
    // NULL compares as NULL, so an unknown BPM, year or rating never matches a bound.
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
    for (col, val) in [
        ("title_norm", &f.title),
        ("artist_norm", &f.artist),
        ("album_norm", &f.album),
    ] {
        if let Some(v) = val {
            sql += &format!(" AND instr(m.{col}, ?) > 0");
            args.push(Value::Text(v.trim().to_lowercase()));
        }
    }
    (sql, args)
}
