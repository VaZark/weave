//! Translates a `FilterSpec` into parameterized SQL. No user text is interpolated.

use rusqlite::types::Value;
use weave_core::{FilterSpec, Sort, SortField, TagOp, parse_tag_ref};

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
    pub table: &'static str,
    pub col: &'static str,
    /// The owner's key as the surrounding query spells it.
    pub outer: &'static str,
}

pub const TRACK_TAGS: TagOwner = TagOwner {
    table: "track_tags",
    col: "track_id",
    outer: "m.track_id",
};
pub const ARTIST_TAGS: TagOwner = TagOwner {
    table: "artist_tags",
    col: "artist_id",
    outer: "ar.id",
};
pub const ALBUM_TAGS: TagOwner = TagOwner {
    table: "album_tags",
    col: "album_id",
    outer: "al.id",
};

/// Present on the owner: the tag itself or any descendant (one `?`: the normalized name), or
/// with `exact` an explicit assignment of exactly that tag.
fn has(o: TagOwner, exact: bool) -> String {
    let TagOwner { table, col, outer } = o;
    if exact {
        format!(
            "EXISTS (SELECT 1 FROM {table} tt JOIN tags g ON g.id = tt.tag_id
             WHERE tt.{col} = {outer} AND g.normalized_name = ?)"
        )
    } else {
        format!(
            "EXISTS (SELECT 1 FROM {table} tt WHERE tt.{col} = {outer} AND tt.tag_id IN (
                 WITH RECURSIVE d(id) AS (
                     SELECT id FROM tags WHERE normalized_name = ?
                     UNION
                     SELECT c.id FROM tags c JOIN d ON c.parent_id = d.id)
                 SELECT id FROM d))"
        )
    }
}

/// `(a OR b …)` over alternatives, binding one name per alternative.
fn any_of(o: TagOwner, names: &[String], exact: bool, args: &mut Vec<Value>) -> String {
    let one = has(o, exact);
    args.extend(names.iter().map(|n| Value::Text(n.clone())));
    format!("({})", vec![one; names.len()].join(" OR "))
}

fn tag_condition(
    o: TagOwner,
    op: TagOp,
    groups: &[Vec<String>],
    exact: bool,
    sql: &mut String,
    args: &mut Vec<Value>,
) {
    match op {
        // Every group must hold, each satisfied by any of its alternatives.
        TagOp::All => {
            for g in groups {
                let c = any_of(o, g, exact, args);
                *sql += &format!(" AND {c}");
            }
        }
        TagOp::Any => {
            let flat: Vec<String> = groups.iter().flatten().cloned().collect();
            let c = any_of(o, &flat, exact, args);
            *sql += &format!(" AND {c}");
        }
        TagOp::Exclude => {
            let flat: Vec<String> = groups.iter().flatten().cloned().collect();
            let c = any_of(o, &flat, exact, args);
            *sql += &format!(" AND NOT {c}");
        }
    }
}

/// The tag conditions of a filter (flat lists, clauses, numeric ranges) as ` AND …` terms over
/// the given kind of owner. Expects a normalized, validated filter.
pub fn tag_conditions(f: &FilterSpec, o: TagOwner) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut args: Vec<Value> = Vec::new();

    // Flat lists: a leading `=` on an entry asks for an exact match.
    for (op, list) in [
        (TagOp::All, &f.tag_all),
        (TagOp::Any, &f.tag_any),
        (TagOp::Exclude, &f.tag_exclude),
    ] {
        if list.is_empty() {
            continue;
        }
        let refs: Vec<(bool, String)> = list.iter().map(|t| parse_tag_ref(t)).collect();
        match op {
            TagOp::All => {
                for (exact, name) in refs {
                    tag_condition(o, op, &[vec![name]], exact, &mut sql, &mut args);
                }
            }
            _ => {
                // Mixed exact/descendant entries OR together.
                let parts: Vec<String> = refs
                    .iter()
                    .map(|(exact, name)| {
                        args.push(Value::Text(name.clone()));
                        has(o, *exact)
                    })
                    .collect();
                let not = if op == TagOp::Exclude { "NOT " } else { "" };
                sql += &format!(" AND {not}({})", parts.join(" OR "));
            }
        }
    }
    for c in &f.tag_clauses {
        tag_condition(o, c.op, &c.names(), c.exact, &mut sql, &mut args);
    }
    for r in &f.tag_range {
        let TagOwner { table, col, outer } = o;
        sql += &format!(
            " AND EXISTS (SELECT 1 FROM {table} tt JOIN tags g ON g.id = tt.tag_id
                   WHERE tt.{col} = {outer} AND g.key = ?"
        );
        args.push(Value::Text(r.key.clone()));
        for (op, v) in [(">=", r.min), ("<=", r.max)] {
            if let Some(v) = v {
                sql += &format!(" AND g.value_num {op} ?");
                args.push(Value::Real(v));
            }
        }
        sql += ")";
    }
    (sql, args)
}

/// ` WHERE …` for `FROM`. Expects a normalized, validated filter.
pub fn where_clause(f: &FilterSpec) -> (String, Vec<Value>) {
    let mut sql = String::from(" WHERE 1=1");
    let (tag_sql, mut args) = tag_conditions(f, TRACK_TAGS);
    sql += &tag_sql;
    // NULL compares as NULL, so an unrated track never matches a bound.
    for (col, op, v) in [
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
