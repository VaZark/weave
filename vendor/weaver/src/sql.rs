//! Compiles a [`TagFilter`] into parameterized SQL. No user text is interpolated.

use rusqlite::types::Value;

use crate::{Owner, TagFilter, TagOp, parse_tag_ref};

/// Present on the owner: the tag itself or any descendant (one `?`: the normalized name), or
/// with `exact` an explicit assignment of exactly that tag.
fn has(o: &Owner, outer: &str, exact: bool) -> String {
    let Owner { table, col, .. } = o;
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
fn any_of(o: &Owner, outer: &str, names: &[String], exact: bool, args: &mut Vec<Value>) -> String {
    let one = has(o, outer, exact);
    args.extend(names.iter().map(|n| Value::Text(n.clone())));
    format!("({})", vec![one; names.len()].join(" OR "))
}

fn condition(
    o: &Owner,
    outer: &str,
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
                let c = any_of(o, outer, g, exact, args);
                *sql += &format!(" AND {c}");
            }
        }
        TagOp::Any => {
            let flat: Vec<String> = groups.iter().flatten().cloned().collect();
            let c = any_of(o, outer, &flat, exact, args);
            *sql += &format!(" AND {c}");
        }
        TagOp::Exclude => {
            let flat: Vec<String> = groups.iter().flatten().cloned().collect();
            let c = any_of(o, outer, &flat, exact, args);
            *sql += &format!(" AND NOT {c}");
        }
    }
}

/// The filter as ` AND …` terms over the given kind of owner, whose key the surrounding query
/// spells `outer` (e.g. `m.track_id`). Expects a normalized, validated filter.
pub fn conditions(f: &TagFilter, o: &Owner, outer: &str) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut args: Vec<Value> = Vec::new();

    for block in &f.blocks {
        let mut alternatives = Vec::new();
        for group in block {
            if group.is_empty() {
                continue;
            }
            let (predicate, values) = conditions(group, o, outer);
            alternatives.push(format!("(1=1{predicate})"));
            args.extend(values);
        }
        if !alternatives.is_empty() {
            sql += &format!(" AND ({})", alternatives.join(" OR "));
        }
    }

    // Flat lists: a leading `=` on an entry asks for an exact match.
    for (op, list) in [
        (TagOp::All, &f.all),
        (TagOp::Any, &f.any),
        (TagOp::Exclude, &f.exclude),
    ] {
        if list.is_empty() {
            continue;
        }
        let refs: Vec<(bool, String)> = list.iter().map(|t| parse_tag_ref(t)).collect();
        match op {
            TagOp::All => {
                for (exact, name) in refs {
                    condition(o, outer, op, &[vec![name]], exact, &mut sql, &mut args);
                }
            }
            _ => {
                // Mixed exact/descendant entries OR together.
                let parts: Vec<String> = refs
                    .iter()
                    .map(|(exact, name)| {
                        args.push(Value::Text(name.clone()));
                        has(o, outer, *exact)
                    })
                    .collect();
                let not = if op == TagOp::Exclude { "NOT " } else { "" };
                sql += &format!(" AND {not}({})", parts.join(" OR "));
            }
        }
    }
    for c in &f.clauses {
        condition(o, outer, c.op, &c.names(), c.exact, &mut sql, &mut args);
    }
    (sql, args)
}
