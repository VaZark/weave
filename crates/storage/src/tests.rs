use super::*;

fn scanned(path: &str, title: &str, artist: &str, bpm: Option<f64>) -> ScannedTrack {
    ScannedTrack {
        external_id: path.into(),
        metadata: Metadata {
            title: title.into(),
            artist: Some(artist.into()),
            bpm,
            ..Default::default()
        },
    }
}

fn names(tracks: &[Track]) -> Vec<&str> {
    tracks.iter().map(|t| t.metadata.title.as_str()).collect()
}

fn all(s: &Storage, f: &FilterSpec) -> Vec<Track> {
    s.search(f, Page::ALL).unwrap().tracks
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
    let everything = all(&s, &FilterSpec::default());
    let id = |n: &str| everything.iter().find(|t| t.metadata.title == n).unwrap().id.0.clone();
    s.add_tags(&id("A"), &tags(&["Bachata", "90s"])).unwrap();
    s.add_tags(&id("B"), &tags(&["bachata", "live"])).unwrap();
    s.add_tags(&id("C"), &tags(&["zouk", "90S"])).unwrap();
    (s, everything)
}

#[test]
fn import_is_idempotent() {
    let mut s = Storage::open_in_memory().unwrap();
    let t = [scanned("/x", "X", "Y", None)];
    assert_eq!(s.import_tracks("local", &t).unwrap(), ImportStats { added: 1, skipped: 0 });
    assert_eq!(s.import_tracks("local", &t).unwrap(), ImportStats { added: 0, skipped: 1 });
    assert_eq!(all(&s, &FilterSpec::default()).len(), 1);
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
    assert_eq!(names(&all(&s, &f(&["bachata", "90s"], &[], &[]))), ["A"]);
    assert_eq!(names(&all(&s, &f(&[], &["bachata", "zouk"], &[]))), ["B", "C", "A"]);
    assert_eq!(names(&all(&s, &f(&[], &[], &["live", "zouk"]))), ["A", "D"]);
    assert_eq!(names(&all(&s, &f(&["bachata"], &[], &["live"]))), ["A"]);
}

#[test]
fn tag_match_is_case_insensitive_and_unicode() {
    let (mut s, fx) = fixture();
    let d = fx.iter().find(|t| t.metadata.title == "D").unwrap();
    s.add_tags(&d.id.0, &tags(&["Été"])).unwrap();
    let f = FilterSpec { tag_all: tags(&["ÉTÉ"]), ..Default::default() };
    assert_eq!(names(&all(&s, &f)), ["D"]);
    assert_eq!(s.tags(&d.id.0).unwrap(), ["Été"]);
}

#[test]
fn duplicate_tags_are_not_added_twice() {
    let (mut s, fx) = fixture();
    let a = &fx.iter().find(|t| t.metadata.title == "A").unwrap().id.0;
    assert_eq!(s.add_tags(a, &tags(&["BACHATA", "new"])).unwrap(), 1);
    assert_eq!(s.tags(a).unwrap(), ["90s", "Bachata", "new"]);
}

#[test]
fn bpm_bounds_inclusive_and_unknown_excluded() {
    let (s, _) = fixture();
    let f = FilterSpec { bpm_min: Some(90.0), bpm_max: Some(140.0), ..Default::default() };
    assert_eq!(all(&s, &f).len(), 3);
    let f = FilterSpec { bpm_max: Some(100.0), ..Default::default() };
    assert_eq!(names(&all(&s, &f)), ["B", "A"]);
    let f = FilterSpec { bpm_min: Some(0.0), ..Default::default() };
    assert!(!names(&all(&s, &f)).contains(&"D"));
}

#[test]
fn rejects_inverted_range() {
    let (s, _) = fixture();
    let f = FilterSpec { bpm_min: Some(2.0), bpm_max: Some(1.0), ..Default::default() };
    assert!(matches!(s.search(&f, Page::ALL), Err(Error::InvalidFilter(_))));
}

#[test]
fn empty_groups_do_not_restrict() {
    let (s, _) = fixture();
    let f = FilterSpec { tag_all: tags(&["", " "]), ..Default::default() };
    assert_eq!(all(&s, &f).len(), 4);
}

#[test]
fn metadata_search_is_unicode_case_insensitive_and_injection_safe() {
    let (s, _) = fixture();
    let f = FilterSpec { artist: Some("édith".into()), ..Default::default() };
    assert_eq!(names(&all(&s, &f)), ["D"]);
    let f = FilterSpec { artist: Some("x' OR '1'='1".into()), ..Default::default() };
    assert!(all(&s, &f).is_empty());
    let f = FilterSpec { tag_any: tags(&["x') OR 1=1 --"]), ..Default::default() };
    assert!(all(&s, &f).is_empty());
}

#[test]
fn remove_tags() {
    let (mut s, fx) = fixture();
    let a = &fx.iter().find(|t| t.metadata.title == "A").unwrap().id.0;
    assert_eq!(s.remove_tags(a, &tags(&["BACHATA", "nope"])).unwrap(), 1);
    assert_eq!(s.tags(a).unwrap(), ["90s"]);
    assert!(matches!(s.remove_tags("missing", &tags(&["x"])), Err(Error::NotFound(_))));
}

#[test]
fn playlists_are_live_queries() {
    let (mut s, fx) = fixture();
    let f = FilterSpec { tag_all: tags(&["90s"]), ..Default::default() };
    let p = s.create_playlist("Bachata 90s", &f).unwrap();
    assert!(matches!(s.create_playlist("bachata 90S", &f), Err(Error::AlreadyExists(_))));
    let members = |s: &Storage| all(s, &s.playlist("bachata 90s").unwrap().filter).len();
    assert_eq!(members(&s), 2);
    let d = &fx.iter().find(|t| t.metadata.title == "D").unwrap().id.0;
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
    assert_eq!(all(&s, &FilterSpec::default()).len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pagination_windows_and_total() {
    let (s, _) = fixture();
    let f = FilterSpec::default();
    let p1 = s.search(&f, Page::numbered(1, 3)).unwrap();
    let p2 = s.search(&f, Page::numbered(2, 3)).unwrap();
    let p3 = s.search(&f, Page::numbered(3, 3)).unwrap();
    assert_eq!((p1.total, p1.tracks.len()), (4, 3));
    assert_eq!((p2.total, p2.tracks.len()), (4, 1));
    assert!(p3.tracks.is_empty() && p3.total == 4);
    let mut paged: Vec<_> = p1.tracks.iter().chain(&p2.tracks).map(|t| t.id.clone()).collect();
    let whole: Vec<_> = all(&s, &f).into_iter().map(|t| t.id).collect();
    assert_eq!(paged.len(), 4);
    paged.sort_by_key(|id| whole.iter().position(|w| w == id));
    assert_eq!(paged, whole, "pages are consecutive slices of the full ordering");
    let off = s.search(&f, Page { offset: 2, limit: None }).unwrap();
    assert_eq!(off.tracks.len(), 2);
}

#[test]
fn tracks_carry_tags_and_full_metadata() {
    let mut s = Storage::open_in_memory().unwrap();
    let m = Metadata {
        title: "T".into(),
        artist: Some("A".into()),
        album_artist: Some("AA".into()),
        genre: Some("Bachata".into()),
        composer: Some("C".into()),
        year: Some(1994),
        track_number: Some(3),
        disc_number: Some(1),
        duration_ms: Some(215_000),
        bpm: Some(120.5),
        ..Default::default()
    };
    s.import_tracks("local", &[ScannedTrack { external_id: "/t".into(), metadata: m.clone() }]).unwrap();
    let t = all(&s, &FilterSpec::default()).remove(0);
    s.add_tags(&t.id.0, &tags(&["zz", "Aa"])).unwrap();
    let t = s.track(&t.id.0).unwrap();
    assert_eq!(t.metadata, m);
    assert_eq!(t.tags, ["Aa", "zz"]);
}

#[test]
fn genre_and_year_filters() {
    let mut s = Storage::open_in_memory().unwrap();
    let mk = |p: &str, genre: Option<&str>, year: Option<i32>| ScannedTrack {
        external_id: p.into(),
        metadata: Metadata { title: p.into(), genre: genre.map(Into::into), year, ..Default::default() },
    };
    s.import_tracks("local", &[mk("a", Some("Bachata"), Some(1994)), mk("b", Some("Zouk"), Some(2005)), mk("c", None, None)])
        .unwrap();
    let f = FilterSpec { genre: Some("bach".into()), ..Default::default() };
    assert_eq!(names(&all(&s, &f)), ["a"]);
    let f = FilterSpec { year_min: Some(1994), year_max: Some(2005), ..Default::default() };
    assert_eq!(names(&all(&s, &f)), ["a", "b"]);
    let f = FilterSpec { year_min: Some(2000), year_max: Some(1990), ..Default::default() };
    assert!(s.search(&f, Page::ALL).is_err());
}

#[test]
fn id_prefix_resolution() {
    let (mut s, all_tracks) = fixture();
    let id = &all_tracks[0].id.0;
    assert_eq!(s.track(&id[..8]).unwrap().id.0, *id);
    assert!(matches!(s.track("zzzz"), Err(Error::NotFound(_))));
    assert!(matches!(s.track(""), Err(Error::InvalidInput(_))));
    assert_eq!(s.add_tags(&id[..8], &tags(&["viaprefix"])).unwrap(), 1);
    // Every ID shares at most a few leading hex chars; the empty-ish "0".."f" prefixes may be ambiguous.
    let ambiguous = (0..16u8).map(|d| format!("{d:x}")).find(|p| {
        all_tracks.iter().filter(|t| t.id.0.starts_with(p.as_str())).count() > 1
    });
    if let Some(p) = ambiguous {
        assert!(matches!(s.track(&p), Err(Error::InvalidInput(_))));
    }
}

#[test]
fn queries_use_indexes_on_the_metadata_table() {
    let (s, _) = fixture();
    let plan = |sql: &str| -> String {
        let mut stmt = s.conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(3)).unwrap().map(|r| r.unwrap()).collect::<Vec<_>>().join(" | ")
    };
    let p = plan("SELECT m.track_id FROM track_metadata m WHERE m.bpm >= 90 AND m.bpm <= 140");
    assert!(p.contains("meta_bpm"), "{p}");
    let p = plan(&format!("SELECT m.track_id FROM track_metadata m {} LIMIT 10", query::ORDER));
    assert!(p.contains("meta_sort") && !p.contains("TEMP B-TREE"), "{p}");
}

#[test]
fn migration_moves_existing_metadata() {
    let conn = Connection::open_in_memory().unwrap();
    query::register_functions(&conn).unwrap();
    // Build a v1 database by hand, then let Storage migrate it.
    conn.execute_batch(
        "CREATE TABLE tracks (id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT, album TEXT, bpm REAL);
         CREATE TABLE sources (id TEXT PRIMARY KEY, track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            backend_id TEXT NOT NULL, external_id TEXT NOT NULL, UNIQUE (backend_id, external_id));
         CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL, normalized_name TEXT NOT NULL UNIQUE);
         CREATE TABLE track_tags (track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE, PRIMARY KEY (track_id, tag_id));
         CREATE TABLE smart_playlists (id TEXT PRIMARY KEY, name TEXT NOT NULL, normalized_name TEXT NOT NULL UNIQUE, filter_spec TEXT NOT NULL);
         INSERT INTO tracks VALUES ('abc', 'Été', 'Édith', NULL, 101.0);
         PRAGMA user_version = 1;",
    )
    .unwrap();
    let s = Storage::init(conn).unwrap();
    let t = s.track("abc").unwrap();
    assert_eq!((t.metadata.title.as_str(), t.metadata.bpm), ("Été", Some(101.0)));
    let f = FilterSpec { artist: Some("ÉDITH".into()), ..Default::default() };
    assert_eq!(all(&s, &f).len(), 1);
}
