use super::*;

fn scanned(path: &str, title: &str, artist: &str, bpm: Option<f64>) -> ScannedTrack {
    ScannedTrack {
        external_id: path.into(),
        title: title.into(),
        artist: Some(artist.into()),
        album: None,
        bpm,
    }
}

fn names(tracks: &[Track]) -> Vec<&str> {
    tracks.iter().map(|t| t.title.as_str()).collect()
}

fn tags(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// a: bachata,90s @100  b: bachata,live @90  c: zouk,90s @140  d: untagged, no bpm
fn fixture() -> (Storage, Vec<Track>) {
    let mut s = Storage::open_in_memory().unwrap();
    s.import_tracks(
        "local",
        &[
            scanned("/a", "A", "Romeo Santos", Some(100.0)),
            scanned("/b", "B", "Aventura", Some(90.0)),
            scanned("/c", "C", "Kaysha", Some(140.0)),
            scanned("/d", "D", "Édith", None),
        ],
    )
    .unwrap();
    let all = s.search(&FilterSpec::default()).unwrap();
    let id = |n: &str| all.iter().find(|t| t.title == n).unwrap().id.0.clone();
    s.add_tags(&id("A"), &tags(&["Bachata", "90s"])).unwrap();
    s.add_tags(&id("B"), &tags(&["bachata", "live"])).unwrap();
    s.add_tags(&id("C"), &tags(&["zouk", "90S"])).unwrap();
    (s, all)
}

#[test]
fn import_is_idempotent() {
    let mut s = Storage::open_in_memory().unwrap();
    let t = [scanned("/x", "X", "Y", None)];
    assert_eq!(s.import_tracks("local", &t).unwrap(), ImportStats { added: 1, skipped: 0 });
    assert_eq!(s.import_tracks("local", &t).unwrap(), ImportStats { added: 0, skipped: 1 });
    assert_eq!(s.search(&FilterSpec::default()).unwrap().len(), 1);
}

#[test]
fn tag_all_any_none() {
    let (s, _) = fixture();
    let f = |all: &[&str], any: &[&str], none: &[&str]| FilterSpec {
        tag_all: tags(all),
        tag_any: tags(any),
        tag_none: tags(none),
        ..Default::default()
    };
    assert_eq!(names(&s.search(&f(&["bachata", "90s"], &[], &[])).unwrap()), ["A"]);
    assert_eq!(names(&s.search(&f(&[], &["bachata", "zouk"], &[])).unwrap()), ["B", "C", "A"]);
    assert_eq!(names(&s.search(&f(&[], &[], &["live", "zouk"])).unwrap()), ["A", "D"]);
    assert_eq!(names(&s.search(&f(&["bachata"], &[], &["live"])).unwrap()), ["A"]);
}

#[test]
fn tag_match_is_case_insensitive_and_unicode() {
    let (mut s, all) = fixture();
    let d = all.iter().find(|t| t.title == "D").unwrap();
    s.add_tags(&d.id.0, &tags(&["Été"])).unwrap();
    let f = FilterSpec { tag_all: tags(&["ÉTÉ"]), ..Default::default() };
    assert_eq!(names(&s.search(&f).unwrap()), ["D"]);
    assert_eq!(s.tags(&d.id.0).unwrap(), ["Été"]);
}

#[test]
fn duplicate_tags_are_not_added_twice() {
    let (mut s, all) = fixture();
    let a = &all.iter().find(|t| t.title == "A").unwrap().id.0;
    assert_eq!(s.add_tags(a, &tags(&["BACHATA", "new"])).unwrap(), 1);
    assert_eq!(s.tags(a).unwrap(), ["90s", "Bachata", "new"]);
}

#[test]
fn bpm_bounds_inclusive_and_unknown_excluded() {
    let (s, _) = fixture();
    let f = FilterSpec { bpm_min: Some(90.0), bpm_max: Some(140.0), ..Default::default() };
    assert_eq!(s.search(&f).unwrap().len(), 3);
    let f = FilterSpec { bpm_max: Some(100.0), ..Default::default() };
    assert_eq!(names(&s.search(&f).unwrap()), ["B", "A"]);
    let f = FilterSpec { bpm_min: Some(0.0), ..Default::default() };
    assert!(!names(&s.search(&f).unwrap()).contains(&"D"));
}

#[test]
fn rejects_inverted_range() {
    let (s, _) = fixture();
    let f = FilterSpec { bpm_min: Some(2.0), bpm_max: Some(1.0), ..Default::default() };
    assert!(matches!(s.search(&f), Err(Error::InvalidFilter(_))));
}

#[test]
fn empty_groups_do_not_restrict() {
    let (s, _) = fixture();
    let f = FilterSpec { tag_all: tags(&["", " "]), ..Default::default() };
    assert_eq!(s.search(&f).unwrap().len(), 4);
}

#[test]
fn metadata_search_is_unicode_case_insensitive_and_injection_safe() {
    let (s, _) = fixture();
    let f = FilterSpec { artist: Some("édith".into()), ..Default::default() };
    assert_eq!(names(&s.search(&f).unwrap()), ["D"]);
    let f = FilterSpec { artist: Some("x' OR '1'='1".into()), ..Default::default() };
    assert!(s.search(&f).unwrap().is_empty());
    let f = FilterSpec { tag_any: tags(&["x') OR 1=1 --"]), ..Default::default() };
    assert!(s.search(&f).unwrap().is_empty());
}

#[test]
fn remove_tags() {
    let (mut s, all) = fixture();
    let a = &all.iter().find(|t| t.title == "A").unwrap().id.0;
    assert_eq!(s.remove_tags(a, &tags(&["BACHATA", "nope"])).unwrap(), 1);
    assert_eq!(s.tags(a).unwrap(), ["90s"]);
    assert!(matches!(s.remove_tags("missing", &tags(&["x"])), Err(Error::NotFound(_))));
}

#[test]
fn playlists_are_live_queries() {
    let (mut s, all) = fixture();
    let f = FilterSpec { tag_all: tags(&["90s"]), ..Default::default() };
    let p = s.create_playlist("Bachata 90s", &f).unwrap();
    assert!(matches!(s.create_playlist("bachata 90S", &f), Err(Error::AlreadyExists(_))));
    let members = |s: &Storage| s.search(&s.playlist("bachata 90s").unwrap().filter).unwrap().len();
    assert_eq!(members(&s), 2);
    let d = &all.iter().find(|t| t.title == "D").unwrap().id.0;
    s.add_tags(d, &tags(&["90s"])).unwrap();
    assert_eq!(members(&s), 3);
    s.update_playlist_filter(&p.id, &FilterSpec { bpm_min: Some(95.0), ..f }).unwrap();
    assert_eq!(members(&s), 2);
    assert_eq!(s.playlists().unwrap().len(), 1);
    assert!(matches!(s.playlist("nope"), Err(Error::NotFound(_))));
}

#[test]
fn migrations_are_idempotent_on_reopen() {
    let dir = std::env::temp_dir().join(format!("weave-test-{}", uuid::Uuid::new_v4().simple()));
    let path = dir.join("lib.db");
    {
        let mut s = Storage::open(&path).unwrap();
        s.import_tracks("local", &[scanned("/x", "X", "Y", None)]).unwrap();
    }
    let s = Storage::open(&path).unwrap();
    assert_eq!(s.search(&FilterSpec::default()).unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}
