//! Dynamic playlists that draw on other playlists: OR across the sources, AND with the filter.

use super::*;
use weave_core::{Sort, SortField};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn find(st: &Storage, f: &FilterSpec) -> Vec<String> {
    let mut v: Vec<String> = st
        .search(f, Page::ALL)
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.metadata.title.clone())
        .collect();
    v.sort();
    v
}

fn of(playlist: &Storage, name: &str) -> Vec<String> {
    find(playlist, &playlist.playlist(name).unwrap().filter)
}

fn from(names: &[&str]) -> FilterSpec {
    FilterSpec {
        sources: s(names),
        ..Default::default()
    }
}

/// A..F with BPM 80, 90, 100, 110, 120, 130. calm: A B C. lively: D E F. Tags: A C E are `es`.
fn fixture() -> Storage {
    let mut st = Storage::open_in_memory().unwrap();
    let tracks: Vec<ScannedTrack> = ["A", "B", "C", "D", "E", "F"]
        .iter()
        .enumerate()
        .map(|(i, t)| ScannedTrack {
            path: format!("/{t}"),
            metadata: Metadata {
                title: (*t).into(),
                artist: Some("x".into()),
                bpm: Some(80.0 + 10.0 * i as f64),
                ..Default::default()
            },
        })
        .collect();
    st.import_tracks(&tracks).unwrap();
    for t in ["A", "C", "E"] {
        st.add_tags(&format!("x-{}", t.to_lowercase()), &s(&["lang:es"]))
            .unwrap();
    }
    for t in ["A", "B", "C"] {
        st.add_tags(&format!("x-{}", t.to_lowercase()), &s(&["mood:calm"]))
            .unwrap();
    }
    st
}

fn manual(st: &mut Storage, name: &str, tracks: &[&str]) {
    st.create_manual_playlist(name, None).unwrap();
    let ids: Vec<String> = tracks
        .iter()
        .map(|t| format!("x-{}", t.to_lowercase()))
        .collect();
    st.playlist_add(name, &ids).unwrap();
}

#[test]
fn a_manual_source_limits_the_candidates_and_the_filter_runs_over_them() {
    let mut st = fixture();
    manual(&mut st, "Picks", &["A", "B", "E"]);
    assert_eq!(find(&st, &from(&["Picks"])), ["A", "B", "E"]);
    let es = FilterSpec {
        sources: s(&["Picks"]),
        tag_all: s(&["lang:es"]),
        ..Default::default()
    };
    assert_eq!(
        find(&st, &es),
        ["A", "E"],
        "C is Spanish but not in the picks"
    );
    st.create_playlist("Picked Spanish", None, &es).unwrap();
    assert_eq!(of(&st, "picked spanish"), ["A", "E"]);
    // the playlist follows its source
    st.playlist_add("Picks", &["x-c".into()]).unwrap();
    assert_eq!(of(&st, "picked spanish"), ["A", "C", "E"]);
}

#[test]
fn a_dynamic_source_contributes_what_it_matches_now() {
    let mut st = fixture();
    st.create_playlist(
        "Calm",
        None,
        &FilterSpec {
            tag_all: s(&["mood:calm"]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(find(&st, &from(&["calm"])), ["A", "B", "C"]);
    let fast_calm = FilterSpec {
        sources: s(&["Calm"]),
        bpm_min: Some(100.0),
        ..Default::default()
    };
    assert_eq!(find(&st, &fast_calm), ["C"]);
    // a new tag changes both the source and what draws on it
    st.add_tags("x-d", &s(&["mood:calm"])).unwrap();
    assert_eq!(find(&st, &from(&["calm"])), ["A", "B", "C", "D"]);
    assert_eq!(find(&st, &fast_calm), ["C", "D"]);
}

#[test]
fn several_sources_are_ored_and_then_anded_with_the_filter() {
    let mut st = fixture();
    manual(&mut st, "Left", &["A", "B"]);
    manual(&mut st, "Right", &["B", "F"]);
    assert_eq!(
        find(&st, &from(&["Left", "Right"])),
        ["A", "B", "F"],
        "a track in any source"
    );
    let both = FilterSpec {
        sources: s(&["Left", "Right"]),
        bpm_min: Some(90.0),
        ..Default::default()
    };
    assert_eq!(
        find(&st, &both),
        ["B", "F"],
        "then the filter applies on top"
    );
    // sources mixing a manual and a dynamic playlist
    st.create_playlist(
        "Spanish",
        None,
        &FilterSpec {
            tag_all: s(&["lang:es"]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        find(&st, &from(&["Right", "Spanish"])),
        ["A", "B", "C", "E", "F"]
    );
    let mixed = FilterSpec {
        sources: s(&["Right", "Spanish"]),
        tag_exclude: s(&["mood:calm"]),
        ..Default::default()
    };
    assert_eq!(find(&st, &mixed), ["E", "F"]);
}

#[test]
fn a_source_with_a_limit_contributes_only_what_the_limit_keeps() {
    let mut st = fixture();
    st.create_playlist(
        "Top two",
        None,
        &FilterSpec {
            sort: Some(Sort {
                field: SortField::Bpm,
                descending: true,
            }),
            limit: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(find(&st, &from(&["Top two"])), ["E", "F"]);
    // the limit applies to the source itself, before this playlist's own filter
    let low = FilterSpec {
        sources: s(&["Top two"]),
        bpm_max: Some(100.0),
        ..Default::default()
    };
    assert!(find(&st, &low).is_empty());
    // a limited source that matches nothing contributes nothing
    st.create_playlist(
        "None",
        None,
        &FilterSpec {
            tag_all: s(&["nope"]),
            limit: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(find(&st, &from(&["None"])).is_empty());
}

#[test]
fn sources_nest_through_dynamic_playlists() {
    let mut st = fixture();
    manual(&mut st, "Picks", &["A", "B", "D", "E"]);
    st.create_playlist(
        "Spanish picks",
        None,
        &FilterSpec {
            sources: s(&["Picks"]),
            tag_all: s(&["lang:es"]),
            ..Default::default()
        },
    )
    .unwrap();
    let top = FilterSpec {
        sources: s(&["Spanish picks"]),
        bpm_min: Some(100.0),
        ..Default::default()
    };
    st.create_playlist("Fast Spanish picks", None, &top)
        .unwrap();
    assert_eq!(of(&st, "fast spanish picks"), ["E"]);
    assert_eq!(find(&st, &from(&["Fast Spanish picks"])), ["E"]);
}

#[test]
fn sources_are_saved_as_ids_and_must_exist() {
    let mut st = fixture();
    manual(&mut st, "Road Trip", &["A"]);
    let p = st
        .create_playlist("Uses it", None, &from(&["ROAD trip"]))
        .unwrap();
    assert_eq!(p.filter.sources, ["road-trip"]);
    assert_eq!(
        st.playlist("uses it").unwrap().filter.sources,
        ["road-trip"]
    );
    assert!(matches!(
        st.create_playlist("Bad", None, &from(&["nope"])),
        Err(Error::NotFound(_))
    ));
    assert!(st.playlist("bad").is_err(), "nothing was saved");
}

#[test]
fn a_playlist_cannot_draw_on_itself_directly_or_through_others() {
    let mut st = fixture();
    let a = st
        .create_playlist("One", None, &FilterSpec::default())
        .unwrap();
    let b = st.create_playlist("Two", None, &from(&["one"])).unwrap();
    let c = st.create_playlist("Three", None, &from(&["two"])).unwrap();
    let err = |r: Result<FilterSpec>| r.unwrap_err().to_string();
    assert!(err(st.update_playlist(&a.id, None, &from(&["one"]))).contains("itself"));
    assert!(err(st.update_playlist(&a.id, None, &from(&["two"]))).contains("loop"));
    assert!(err(st.update_playlist(&a.id, None, &from(&["three"]))).contains("loop"));
    // nothing changed, and a legitimate change still works
    assert!(st.playlist("one").unwrap().filter.sources.is_empty());
    let saved = st.update_playlist(&b.id, None, &from(&["one"])).unwrap();
    assert_eq!(saved.sources, ["one"]);
    assert_eq!(c.filter.sources, ["two"]);
}

#[test]
fn an_ad_hoc_search_can_draw_on_playlists_by_name_and_a_deep_chain_is_refused() {
    let mut st = fixture();
    manual(&mut st, "Picks", &["B", "C"]);
    assert_eq!(find(&st, &from(&["picks"])), ["B", "C"]);
    assert!(st.search(&from(&["missing"]), Page::ALL).is_err());
    // a chain longer than the limit is an error, not a hang
    st.create_playlist("L0", None, &FilterSpec::default())
        .unwrap();
    for i in 1..=10 {
        st.create_playlist(&format!("L{i}"), None, &from(&[&format!("L{}", i - 1)]))
            .unwrap();
    }
    let msg = st
        .search(&from(&["L10"]), Page::ALL)
        .unwrap_err()
        .to_string();
    assert!(msg.contains("levels deep"), "{msg}");
}
