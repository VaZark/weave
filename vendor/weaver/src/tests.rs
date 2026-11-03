use rusqlite::{Connection, params_from_iter};

use super::*;

const ITEM: Owner = Owner {
    name: "item",
    table: "item_tags",
    col: "item_id",
    owner_table: "items",
    owner_key: "id",
    key_type: "TEXT",
};
const GROUP: Owner = Owner {
    name: "group",
    table: "group_tags",
    col: "group_id",
    owner_table: "groups",
    owner_key: "id",
    key_type: "INTEGER",
};
static OWNERS: [Owner; 2] = [ITEM, GROUP];
const TAGS: Tags = Tags::new(&OWNERS);

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// genre:bachata > genre:bachata-traditional, genre:bachata-sensual; items T, S, B, Z.
fn fixture() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE items (id TEXT PRIMARY KEY); CREATE TABLE groups (id INTEGER PRIMARY KEY);")
        .unwrap();
    db.execute_batch(&TAGS.schema_sql()).unwrap();
    for id in ["T", "S", "B", "Z"] {
        db.execute("INSERT INTO items VALUES (?1)", [id]).unwrap();
    }
    TAGS.create(&mut db, "genre:bachata", None).unwrap();
    TAGS.create(&mut db, "genre:bachata-traditional", Some("genre:bachata"))
        .unwrap();
    TAGS.create(&mut db, "genre:bachata-sensual", Some("genre:bachata"))
        .unwrap();
    for (item, tag) in [
        ("T", "genre:bachata-traditional"),
        ("S", "genre:bachata-sensual"),
        ("B", "genre:bachata"),
        ("Z", "genre:zouk"),
    ] {
        TAGS.add(&mut db, &ITEM, item, &s(&[tag])).unwrap();
    }
    db
}

fn find(db: &Connection, f: TagFilter) -> Vec<String> {
    let (sql, args) = conditions(&f.normalized(), &ITEM, "i.id");
    let mut stmt = db
        .prepare(&format!(
            "SELECT i.id FROM items i WHERE 1=1{sql} ORDER BY i.id"
        ))
        .unwrap();
    stmt.query_map(params_from_iter(args), |r| r.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
}

fn clause(op: TagOp, keys: &[&str], values: &[&str], exact: bool) -> TagFilter {
    TagFilter {
        clauses: vec![TagClause {
            op,
            keys: s(keys),
            values: s(values),
            exact,
        }],
        ..Default::default()
    }
}

#[test]
fn descendants_exact_and_exclusion() {
    let db = fixture();
    let all = |t: &str| TagFilter {
        all: s(&[t]),
        ..Default::default()
    };
    assert_eq!(find(&db, all("genre:bachata")), ["B", "S", "T"]);
    assert_eq!(find(&db, all("=genre:bachata")), ["B"]);
    assert_eq!(find(&db, all("GENRE:Zouk")), ["Z"]);
    let none = TagFilter {
        exclude: s(&["genre:bachata"]),
        ..Default::default()
    };
    assert_eq!(find(&db, none), ["Z"]);
    let any = TagFilter {
        any: s(&["genre:zouk", "=genre:bachata"]),
        ..Default::default()
    };
    assert_eq!(find(&db, any), ["B", "Z"]);
    assert_eq!(find(&db, TagFilter::default()), ["B", "S", "T", "Z"]);
}

#[test]
fn keyed_clauses() {
    let mut db = fixture();
    TAGS.add(&mut db, &ITEM, "S", &s(&["style:salsa"])).unwrap();
    let f = clause(TagOp::Any, &["genre", "style"], &["salsa", "zouk"], false);
    assert_eq!(find(&db, f), ["S", "Z"]);
    let f = clause(TagOp::All, &["genre", "style"], &["zouk", "salsa"], false);
    assert!(find(&db, f).is_empty());
    let f = clause(TagOp::Exclude, &["genre"], &["bachata"], false);
    assert_eq!(find(&db, f), ["Z"]);
    let f = clause(TagOp::Any, &["genre"], &["bachata"], true);
    assert_eq!(find(&db, f), ["B"]);
}

#[test]
fn tree_rules() {
    let mut db = fixture();
    assert!(matches!(
        TAGS.set_parent(&mut db, "genre:bachata", Some("genre:bachata")),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        TAGS.set_parent(&mut db, "genre:bachata", Some("genre:bachata-sensual")),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        TAGS.create(&mut db, "GENRE:bachata", None),
        Err(Error::AlreadyExists(_))
    ));
    let t = TAGS
        .set_parent(&mut db, "genre:zouk", Some("genre:bachata"))
        .unwrap();
    assert_eq!(t.parent.as_deref(), Some("genre:bachata"));

    assert_eq!(
        TAGS.delete(&mut db, "genre:bachata", ChildPolicy::Detach)
            .unwrap(),
        1
    );
    assert_eq!(TAGS.get(&db, "genre:zouk").unwrap().parent, None);
    assert_eq!(TAGS.of(&db, &ITEM, "B").unwrap(), Vec::<String>::new());
    assert_eq!(
        TAGS.delete(&mut db, "genre:zouk", ChildPolicy::Delete)
            .unwrap(),
        1
    );
}

#[test]
fn owners_share_one_vocabulary() {
    let mut db = fixture();
    db.execute("INSERT INTO groups VALUES (7)", []).unwrap();
    TAGS.add(&mut db, &GROUP, "7", &s(&["genre:zouk"])).unwrap();
    let t = TAGS.get(&db, "genre:zouk").unwrap();
    assert_eq!((t.count("item"), t.count("group")), (1, 1));
    assert_eq!(TAGS.of(&db, &GROUP, "7").unwrap(), ["genre:zouk"]);
    assert_eq!(
        TAGS.add(&mut db, &ITEM, "Z", &s(&["GENRE:zouk"])).unwrap(),
        0
    );
}

#[test]
fn copy_merges_assignments() {
    let mut db = fixture();
    let tx = db.transaction().unwrap();
    assert_eq!(TAGS.copy(&tx, &ITEM, "T", "Z").unwrap(), 1);
    tx.commit().unwrap();
    assert_eq!(
        TAGS.of(&db, &ITEM, "Z").unwrap(),
        ["genre:bachata-traditional", "genre:zouk"]
    );
}

#[test]
fn filter_normalizing_and_validation() {
    let f = TagFilter {
        all: s(&[" Bachata ", "bachata", "", "=Bachata"]),
        clauses: vec![TagClause {
            op: TagOp::Any,
            keys: s(&["Genre", "genre"]),
            values: s(&[" x "]),
            exact: false,
        }],
        ..Default::default()
    }
    .normalized();
    assert_eq!(f.all, ["Bachata", "=Bachata"]);
    assert_eq!(f.clauses[0].keys, ["genre"]);
    assert!(f.validate().is_ok());
    let bad = clause(TagOp::Any, &["not a key"], &["x"], false);
    assert!(bad.validate().is_err());
}

#[test]
fn a_tag_has_an_icon_and_a_color() {
    let mut db = fixture();
    let t = TAGS.get(&db, "genre:zouk").unwrap();
    assert_eq!((t.icon, t.color), (None, None));
    let t = TAGS
        .set_style(&mut db, "GENRE:Zouk", Some(" moon "), Some("#C0F"))
        .unwrap();
    assert_eq!(t.icon.as_deref(), Some("moon"));
    assert_eq!(t.color.as_deref(), Some("#cc00ff"));
    // part of it can be cleared, and listing carries it
    TAGS.set_style(&mut db, "genre:zouk", None, Some("#112233"))
        .unwrap();
    let listed = TAGS.list(&db).unwrap();
    let z = listed.iter().find(|t| t.name == "genre:zouk").unwrap();
    assert_eq!(
        (z.icon.as_deref(), z.color.as_deref()),
        (None, Some("#112233"))
    );
    // invalid colours are rejected and nothing changes
    for bad in ["red", "#12", "#ggg", "123456", ""] {
        assert!(
            matches!(
                TAGS.set_style(&mut db, "genre:zouk", Some("x"), Some(bad)),
                Err(Error::InvalidInput(_))
            ),
            "{bad}"
        );
    }
    assert_eq!(TAGS.get(&db, "genre:zouk").unwrap().icon, None);
    assert!(matches!(
        TAGS.set_style(&mut db, "nope", None, None),
        Err(Error::NotFound(_))
    ));
}
