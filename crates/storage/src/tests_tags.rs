//! Tag hierarchy, keyed tags, ranges, provenance.

use super::*;
use weave_core::{ChildPolicy, TagClause, TagOp, TagRange};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn find(st: &Storage, f: FilterSpec) -> Vec<String> {
    let mut v: Vec<String> = st
        .search(&f, Page::ALL)
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.metadata.title.clone())
        .collect();
    v.sort();
    v
}

fn id(st: &Storage, title: &str) -> String {
    st.search(&FilterSpec::default(), Page::ALL)
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.metadata.title == title)
        .unwrap()
        .id
        .0
        .clone()
}

/// genre:bachata > genre:bachata-traditional, genre:bachata-sensual
/// T: traditional  S: sensual  B: bachata itself  Z: zouk  (all with bpm tags)
fn fixture() -> Storage {
    let mut st = Storage::open_in_memory().unwrap();
    let t = |n: &str, bpm| ScannedTrack {
        path: n.into(),
        metadata: Metadata {
            title: n.into(),
            bpm: Some(bpm),
            ..Default::default()
        },
    };
    st.import_tracks(&[t("T", 90.0), t("S", 110.0), t("B", 130.0), t("Z", 150.0)])
        .unwrap();
    st.create_tag("genre:bachata", None).unwrap();
    st.create_tag("genre:bachata-traditional", Some("genre:bachata"))
        .unwrap();
    st.create_tag("genre:bachata-sensual", Some("genre:bachata"))
        .unwrap();
    for (track, tag) in [
        ("T", "genre:bachata-traditional"),
        ("S", "genre:bachata-sensual"),
        ("B", "genre:bachata"),
        ("Z", "genre:zouk"),
    ] {
        let i = id(&st, track);
        st.add_tags(&i, &s(&[tag])).unwrap();
    }
    st
}

#[test]
fn has_includes_descendants_has_exact_does_not_and_none_excludes_them() {
    let st = fixture();
    let f = |all: &[&str], none: &[&str]| FilterSpec {
        tag_all: s(all),
        tag_exclude: s(none),
        ..Default::default()
    };
    assert_eq!(find(&st, f(&["genre:bachata"], &[])), ["B", "S", "T"]);
    assert_eq!(find(&st, f(&["=genre:bachata"], &[])), ["B"]);
    assert_eq!(
        find(&st, f(&["genre:bachata"], &["genre:bachata-traditional"])),
        ["B", "S"]
    );
    assert_eq!(find(&st, f(&[], &["genre:bachata"])), ["Z"]);
    // assignments stayed flat: traditional does not also carry bachata
    let i = id(&st, "T");
    assert_eq!(
        st.tags(&i).unwrap(),
        ["bpm:90", "genre:bachata-traditional"]
    );
}

#[test]
fn clauses_with_keys_ops_and_multiple_keys() {
    let mut st = fixture();
    let i = id(&st, "Z");
    st.add_tags(&i, &s(&["style:salsa"])).unwrap();
    let clause = |op, keys: &[&str], values: &[&str], exact| FilterSpec {
        tag_clauses: vec![TagClause {
            op,
            keys: s(keys),
            values: s(values),
            exact,
        }],
        ..Default::default()
    };
    let any = clause(TagOp::Any, &["genre"], &["bachata", "zouk"], false);
    assert_eq!(find(&st, any), ["B", "S", "T", "Z"]);
    let exact = clause(TagOp::Any, &["genre"], &["bachata"], true);
    assert_eq!(find(&st, exact), ["B"]);
    let excl = clause(TagOp::Exclude, &["genre"], &["bachata"], false);
    assert_eq!(find(&st, excl), ["Z"]);
    // two keys: a value counts when either key has it
    let keys = clause(TagOp::Any, &["genre", "style"], &["salsa"], false);
    assert_eq!(find(&st, keys), ["Z"]);
    // all: every value, each from any key
    let all = clause(TagOp::All, &["genre", "style"], &["zouk", "salsa"], false);
    assert_eq!(find(&st, all), ["Z"]);
    // clauses AND together
    let mut both = clause(TagOp::Any, &["genre"], &["bachata"], false);
    both.tag_clauses
        .extend(clause(TagOp::Exclude, &["genre"], &["bachata-sensual"], false).tag_clauses);
    assert_eq!(find(&st, both), ["B", "T"]);
}

#[test]
fn reparenting_changes_queries_not_assignments() {
    let mut st = fixture();
    let q = || FilterSpec {
        tag_all: s(&["genre:bachata"]),
        ..Default::default()
    };
    st.set_tag_parent("genre:bachata-sensual", None).unwrap();
    assert_eq!(find(&st, q()), ["B", "T"]);
    st.set_tag_parent("genre:zouk", Some("genre:bachata"))
        .unwrap();
    assert_eq!(find(&st, q()), ["B", "T", "Z"]);
    assert_eq!(
        st.tags(&id(&st, "S")).unwrap(),
        ["bpm:110", "genre:bachata-sensual"]
    );
}

#[test]
fn cycles_and_self_parenting_are_rejected() {
    let mut st = fixture();
    let err = |r: Result<weave_core::Tag>| r.unwrap_err().to_string();
    assert!(err(st.set_tag_parent("genre:bachata", Some("genre:bachata"))).contains("own parent"));
    assert!(
        err(st.set_tag_parent("genre:bachata", Some("genre:bachata-sensual"))).contains("cycle")
    );
    st.create_tag("genre:deep", Some("genre:bachata-sensual"))
        .unwrap();
    assert!(err(st.set_tag_parent("genre:bachata", Some("genre:deep"))).contains("cycle"));
    // nothing changed
    assert_eq!(st.tag("genre:bachata").unwrap().parent, None);
    assert_eq!(
        st.tag("genre:deep").unwrap().parent.as_deref(),
        Some("genre:bachata-sensual")
    );
    assert!(matches!(
        st.create_tag("genre:bachata", None),
        Err(Error::AlreadyExists(_))
    ));
    assert!(matches!(
        st.create_tag("x", Some("missing")),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn deleting_a_parent_detaches_or_deletes_children() {
    let mut st = fixture();
    assert_eq!(
        st.delete_tag("genre:bachata", ChildPolicy::Detach).unwrap(),
        1
    );
    assert_eq!(st.tag("genre:bachata-sensual").unwrap().parent, None);
    assert_eq!(st.tags(&id(&st, "B")).unwrap(), ["bpm:130"]);
    assert_eq!(
        st.tags(&id(&st, "S")).unwrap(),
        ["bpm:110", "genre:bachata-sensual"]
    );

    let mut st = fixture();
    assert_eq!(
        st.delete_tag("genre:bachata", ChildPolicy::Delete).unwrap(),
        3
    );
    assert!(st.tag("genre:bachata-traditional").is_err());
    assert_eq!(st.tags(&id(&st, "T")).unwrap(), ["bpm:90"]);
    assert_eq!(st.tags(&id(&st, "Z")).unwrap(), ["bpm:150", "genre:zouk"]);
}

#[test]
fn numeric_ranges_compare_numbers_not_text() {
    let mut st = fixture();
    for (track, bpm) in [
        ("T", "bpm:90"),
        ("S", "bpm:110"),
        ("B", "bpm:130"),
        ("Z", "bpm:1000"),
    ] {
        let i = id(&st, track);
        st.add_tags(&i, &s(&[bpm])).unwrap();
    }
    let r = |min, max| FilterSpec {
        tag_range: vec![TagRange {
            key: "bpm".into(),
            min,
            max,
        }],
        ..Default::default()
    };
    // text order would put "1000" before "90"
    assert_eq!(find(&st, r(Some(90.0), Some(120.0))), ["S", "T"]);
    assert_eq!(find(&st, r(Some(500.0), None)), ["Z"]);
    assert!(
        r(Some(5.0), Some(1.0)).validate().is_err()
            && r(None, None).validate().is_err()
            && FilterSpec {
                tag_range: vec![TagRange {
                    key: "no good".into(),
                    min: Some(1.0),
                    max: None
                }],
                ..Default::default()
            }
            .validate()
            .is_err()
    );
    assert!(st.add_tags(&id(&st, "T"), &s(&["bpm:fast"])).is_err());
}

#[test]
fn import_turns_metadata_into_ordinary_tags() {
    let mut st = Storage::open_in_memory().unwrap();
    let t = ScannedTrack {
        path: "/a".into(),
        metadata: Metadata {
            title: "A".into(),
            artist: Some("Aventura".into()),
            genre: Some("Bachata".into()),
            year: Some(1999),
            bpm: Some(90.0),
            ..Default::default()
        },
    };
    st.import_tracks(std::slice::from_ref(&t)).unwrap();
    let i = id(&st, "A");
    // genre, year and bpm only: artist (like title and album) identifies the track
    assert_eq!(
        st.tags(&i).unwrap(),
        ["bpm:90", "genre:Bachata", "year:1999"]
    );
    let by_genre = FilterSpec {
        tag_all: s(&["genre:bachata"]),
        ..Default::default()
    };
    assert_eq!(find(&st, by_genre), ["A"]);
    // adding what is already there is a no-op; a re-import changes nothing
    assert_eq!(st.add_tags(&i, &s(&["GENRE:bachata"])).unwrap(), 0);
    st.add_tags(&i, &s(&["practice"])).unwrap();
    assert_eq!(st.import_tracks(&[t]).unwrap().added, 0);
    assert_eq!(
        st.tags(&i).unwrap(),
        ["bpm:90", "genre:Bachata", "practice", "year:1999"]
    );
}
