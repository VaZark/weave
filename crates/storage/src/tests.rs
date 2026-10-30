use super::*;
use weave_core::{EndOfQueue, PlayerState, PlaylistKind, Queue, RepeatMode, Rng, Sort, SortField};

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
    let id = |n: &str| {
        everything
            .iter()
            .find(|t| t.metadata.title == n)
            .unwrap()
            .id
            .0
            .clone()
    };
    s.add_tags(&id("A"), &tags(&["Bachata", "90s"])).unwrap();
    s.add_tags(&id("B"), &tags(&["bachata", "live"])).unwrap();
    s.add_tags(&id("C"), &tags(&["zouk", "90S"])).unwrap();
    (s, everything)
}

#[test]
fn import_is_idempotent() {
    let mut s = Storage::open_in_memory().unwrap();
    let t = [scanned("/x", "X", "Y", None)];
    assert_eq!(
        s.import_tracks("local", &t).unwrap(),
        ImportStats {
            added: 1,
            skipped: 0
        }
    );
    assert_eq!(
        s.import_tracks("local", &t).unwrap(),
        ImportStats {
            added: 0,
            skipped: 1
        }
    );
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
    assert_eq!(
        names(&all(&s, &f(&[], &["bachata", "zouk"], &[]))),
        ["B", "C", "A"]
    );
    assert_eq!(names(&all(&s, &f(&[], &[], &["live", "zouk"]))), ["A", "D"]);
    assert_eq!(names(&all(&s, &f(&["bachata"], &[], &["live"]))), ["A"]);
}

#[test]
fn tag_match_is_case_insensitive_and_unicode() {
    let (mut s, fx) = fixture();
    let d = fx.iter().find(|t| t.metadata.title == "D").unwrap();
    s.add_tags(&d.id.0, &tags(&["Été"])).unwrap();
    let f = FilterSpec {
        tag_all: tags(&["ÉTÉ"]),
        ..Default::default()
    };
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
    let f = FilterSpec {
        bpm_min: Some(90.0),
        bpm_max: Some(140.0),
        ..Default::default()
    };
    assert_eq!(all(&s, &f).len(), 3);
    let f = FilterSpec {
        bpm_max: Some(100.0),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["B", "A"]);
    let f = FilterSpec {
        bpm_min: Some(0.0),
        ..Default::default()
    };
    assert!(!names(&all(&s, &f)).contains(&"D"));
}

#[test]
fn rejects_inverted_range() {
    let (s, _) = fixture();
    let f = FilterSpec {
        bpm_min: Some(2.0),
        bpm_max: Some(1.0),
        ..Default::default()
    };
    assert!(matches!(
        s.search(&f, Page::ALL),
        Err(Error::InvalidFilter(_))
    ));
}

#[test]
fn empty_groups_do_not_restrict() {
    let (s, _) = fixture();
    let f = FilterSpec {
        tag_all: tags(&["", " "]),
        ..Default::default()
    };
    assert_eq!(all(&s, &f).len(), 4);
}

#[test]
fn metadata_search_is_unicode_case_insensitive_and_injection_safe() {
    let (s, _) = fixture();
    let f = FilterSpec {
        artist: Some("édith".into()),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["D"]);
    let f = FilterSpec {
        artist: Some("x' OR '1'='1".into()),
        ..Default::default()
    };
    assert!(all(&s, &f).is_empty());
    let f = FilterSpec {
        tag_any: tags(&["x') OR 1=1 --"]),
        ..Default::default()
    };
    assert!(all(&s, &f).is_empty());
}

#[test]
fn remove_tags() {
    let (mut s, fx) = fixture();
    let a = &fx.iter().find(|t| t.metadata.title == "A").unwrap().id.0;
    assert_eq!(s.remove_tags(a, &tags(&["BACHATA", "nope"])).unwrap(), 1);
    assert_eq!(s.tags(a).unwrap(), ["90s"]);
    assert!(matches!(
        s.remove_tags("missing", &tags(&["x"])),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn playlists_are_live_queries() {
    let (mut s, fx) = fixture();
    let f = FilterSpec {
        tag_all: tags(&["90s"]),
        ..Default::default()
    };
    let p = s
        .create_playlist("Bachata 90s", Some("Slow ones"), &f)
        .unwrap();
    assert!(matches!(
        s.create_playlist("bachata 90S", None, &f),
        Err(Error::AlreadyExists(_))
    ));
    let members = |s: &Storage| all(s, &s.playlist("bachata 90s").unwrap().filter).len();
    assert_eq!(members(&s), 2);
    let d = &fx.iter().find(|t| t.metadata.title == "D").unwrap().id.0;
    s.add_tags(d, &tags(&["90s"])).unwrap();
    assert_eq!(members(&s), 3);
    s.update_playlist(
        &p.id,
        p.description.as_deref(),
        &FilterSpec {
            bpm_min: Some(95.0),
            ..f
        },
    )
    .unwrap();
    assert_eq!(members(&s), 2);
    assert_eq!(s.playlists().unwrap().len(), 1);
    assert_eq!(
        s.playlist("BACHATA 90S").unwrap().description.as_deref(),
        Some("Slow ones")
    );
    assert!(matches!(s.playlist("nope"), Err(Error::NotFound(_))));
}

#[test]
fn migrations_are_idempotent_on_reopen() {
    let dir = std::env::temp_dir().join(format!("weave-test-{}", uuid::Uuid::new_v4().simple()));
    let path = dir.join("lib.db");
    {
        let mut s = Storage::open(&path).unwrap();
        s.import_tracks("local", &[scanned("/x", "X", "Y", None)])
            .unwrap();
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
    let mut paged: Vec<_> = p1
        .tracks
        .iter()
        .chain(&p2.tracks)
        .map(|t| t.id.clone())
        .collect();
    let whole: Vec<_> = all(&s, &f).into_iter().map(|t| t.id).collect();
    assert_eq!(paged.len(), 4);
    paged.sort_by_key(|id| whole.iter().position(|w| w == id));
    assert_eq!(
        paged, whole,
        "pages are consecutive slices of the full ordering"
    );
    let off = s
        .search(
            &f,
            Page {
                offset: 2,
                limit: None,
            },
        )
        .unwrap();
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
    s.import_tracks(
        "local",
        &[ScannedTrack {
            external_id: "/t".into(),
            metadata: m.clone(),
        }],
    )
    .unwrap();
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
        metadata: Metadata {
            title: p.into(),
            genre: genre.map(Into::into),
            year,
            ..Default::default()
        },
    };
    s.import_tracks(
        "local",
        &[
            mk("a", Some("Bachata"), Some(1994)),
            mk("b", Some("Zouk"), Some(2005)),
            mk("c", None, None),
        ],
    )
    .unwrap();
    let f = FilterSpec {
        genre: Some("bach".into()),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["a"]);
    let f = FilterSpec {
        year_min: Some(1994),
        year_max: Some(2005),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["a", "b"]);
    let f = FilterSpec {
        year_min: Some(2000),
        year_max: Some(1990),
        ..Default::default()
    };
    assert!(s.search(&f, Page::ALL).is_err());
}

#[test]
fn internal_key_prefix_still_resolves() {
    let (s, fx) = fixture();
    assert_eq!(fx[0].id.0, "aventura-b-local");
    let internal: String = s
        .conn
        .query_row(
            "SELECT id FROM tracks WHERE slug = ?1",
            [&fx[0].id.0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(s.track(&internal[..8]).unwrap().id, fx[0].id);
    assert!(matches!(s.track("zzzz"), Err(Error::NotFound(_))));
    assert!(matches!(s.track(""), Err(Error::InvalidInput(_))));
}

#[test]
fn queries_use_indexes_on_the_metadata_table() {
    let (s, _) = fixture();
    let plan = |sql: &str| -> String {
        let mut stmt = s
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let p = plan("SELECT m.track_id FROM track_metadata m WHERE m.bpm >= 90 AND m.bpm <= 140");
    assert!(p.contains("meta_bpm"), "{p}");
    // The real paging query (with its joins) must walk the sort index without sorting.
    let p = plan(&format!(
        "SELECT {}{}{} LIMIT 10",
        query::COLUMNS,
        query::FROM,
        query::order_by(None)
    ));
    assert!(p.contains("meta_sort") && !p.contains("TEMP B-TREE"), "{p}");
}

#[test]
fn slugs_are_assigned_unique_and_resolvable() {
    let mut s = Storage::open_in_memory().unwrap();
    let mk = |p: &str, artist: Option<&str>, album: Option<&str>, title: &str| ScannedTrack {
        external_id: p.into(),
        metadata: Metadata {
            title: title.into(),
            artist: artist.map(Into::into),
            album: album.map(Into::into),
            ..Default::default()
        },
    };
    s.import_tracks(
        "local",
        &[
            mk("/1", Some("Romeo Santos"), Some("Fórmula"), "Don't Stop"),
            mk("/2", Some("Romeo Santos"), Some("Fórmula"), "Don't Stop"),
            mk("/3", None, None, "Mystery"),
        ],
    )
    .unwrap();
    // Identical sort keys tie-break on the internal key, so compare as a sorted set.
    let mut slugs: Vec<String> = all(&s, &FilterSpec::default())
        .into_iter()
        .map(|t| t.id.0)
        .collect();
    slugs.sort();
    assert_eq!(
        slugs,
        [
            "romeo-santos-fórmula-dont-stop-local",
            "romeo-santos-fórmula-dont-stop-local-2",
            "unknown-mystery-local",
        ]
    );
    // exact, differently-written, and via tag operations
    assert_eq!(
        s.track("romeo-santos-fórmula-dont-stop-local-2")
            .unwrap()
            .id
            .0,
        slugs[1]
    );
    assert_eq!(s.track("Unknown / Mystery / LOCAL").unwrap().id.0, slugs[2]);
    assert_eq!(
        s.add_tags("unknown-mystery-local", &tags(&["x"])).unwrap(),
        1
    );
    assert!(matches!(
        s.track("nope-nope-local"),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        s.track("x'; DROP TABLE tracks;--"),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn linked_sqlite_meets_minimum() {
    assert!(
        rusqlite::version_number() >= MIN_SQLITE_VERSION,
        "{}",
        rusqlite::version()
    );
}

#[test]
fn playlists_have_unique_slug_ids_and_resolve_by_name_or_slug() {
    let mut s = Storage::open_in_memory().unwrap();
    let f = FilterSpec::default();
    let a = s.create_playlist("Rock & Roll", None, &f).unwrap();
    let b = s.create_playlist("rock and roll!", None, &f).unwrap();
    let c = s.create_playlist("???", None, &f).unwrap();
    assert_eq!(
        (a.id.as_str(), b.id.as_str(), c.id.as_str()),
        ("rock-and-roll", "rock-and-roll-2", "playlist")
    );
    // by name (exact name wins over a slug match), by slug, by sloppy slug
    assert_eq!(s.playlist("rock and roll!").unwrap().id, "rock-and-roll-2");
    assert_eq!(s.playlist("rock-and-roll-2").unwrap().id, "rock-and-roll-2");
    assert_eq!(s.playlist("Rock & Roll").unwrap().id, "rock-and-roll");
    assert_eq!(s.playlist("ROCK-AND-ROLL").unwrap().id, "rock-and-roll");
    assert!(matches!(
        s.create_playlist("ROCK & roll", None, &f),
        Err(Error::AlreadyExists(_))
    ));
    assert!(matches!(s.playlist("nope"), Err(Error::NotFound(_))));
}

fn ids_of(tracks: &[Track]) -> Vec<&str> {
    tracks.iter().map(|t| t.id.0.as_str()).collect()
}

/// Titles with explicit year/bpm/rating to exercise sorting and annotations.
fn annotated() -> Storage {
    let mut s = Storage::open_in_memory().unwrap();
    let mk = |t: &str, artist: &str, year: Option<i32>, bpm: Option<f64>| ScannedTrack {
        external_id: t.into(),
        metadata: Metadata {
            title: t.into(),
            artist: Some(artist.into()),
            year,
            bpm,
            duration_ms: Some(200_000),
            ..Default::default()
        },
    };
    s.import_tracks(
        "local",
        &[
            mk("A", "x", Some(1990), Some(100.0)),
            mk("B", "x", Some(2000), None),
            mk("C", "y", None, Some(80.0)),
            mk("D", "y", Some(1980), Some(120.0)),
        ],
    )
    .unwrap();
    s
}

fn id(s: &Storage, title: &str) -> String {
    all(
        s,
        &FilterSpec {
            title: Some(title.into()),
            ..Default::default()
        },
    )[0]
    .id
    .0
    .clone()
}

#[test]
fn favorites_and_ratings_are_stored_filtered_and_shown() {
    let mut s = annotated();
    let (a, b, c) = (id(&s, "a"), id(&s, "b"), id(&s, "c"));
    assert_eq!(s.set_favorite(&[a.clone(), b.clone()], true).unwrap(), 2);
    assert_eq!(
        s.set_favorite(std::slice::from_ref(&a), true).unwrap(),
        0,
        "already a favorite"
    );
    s.set_rating(&a, Some(5)).unwrap();
    s.set_rating(&c, Some(2)).unwrap();

    let t = s.track(&a).unwrap();
    assert_eq!((t.favorite, t.rating), (true, Some(5)));
    let t = s.track(&id(&s, "d")).unwrap();
    assert_eq!((t.favorite, t.rating), (false, None));

    let f = |favorite, min, max| FilterSpec {
        favorite,
        rating_min: min,
        rating_max: max,
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f(Some(true), None, None))), ["A", "B"]);
    assert_eq!(names(&all(&s, &f(Some(false), None, None))), ["C", "D"]);
    assert_eq!(names(&all(&s, &f(None, Some(3), None))), ["A"]);
    assert_eq!(
        names(&all(&s, &f(None, Some(1), Some(2)))),
        ["C"],
        "unrated never matches a bound"
    );
    assert_eq!(names(&all(&s, &f(Some(true), Some(4), None))), ["A"]);

    assert_eq!(s.set_favorite(std::slice::from_ref(&a), false).unwrap(), 1);
    s.set_rating(&a, None).unwrap();
    let t = s.track(&a).unwrap();
    assert_eq!((t.favorite, t.rating), (false, None));
    assert!(s.set_rating(&a, Some(6)).is_err());
    assert!(s.set_rating(&a, Some(0)).is_err());
}

#[test]
fn unknown_id_changes_no_favorites() {
    let mut s = annotated();
    let a = id(&s, "a");
    assert!(matches!(
        s.set_favorite(&[a.clone(), "nope".into()], true),
        Err(Error::NotFound(_))
    ));
    assert!(!s.track(&a).unwrap().favorite);
}

#[test]
fn sorting_puts_unknowns_last_in_both_directions() {
    let s = annotated();
    let by = |field, descending| {
        let f = FilterSpec {
            sort: Some(Sort { field, descending }),
            ..Default::default()
        };
        names(&all(&s, &f))
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert_eq!(by(SortField::Year, false), ["D", "A", "B", "C"]);
    assert_eq!(by(SortField::Year, true), ["B", "A", "D", "C"]);
    assert_eq!(by(SortField::Bpm, false), ["C", "A", "D", "B"]);
    assert_eq!(by(SortField::Bpm, true), ["D", "A", "C", "B"]);
    assert_eq!(by(SortField::Title, true), ["D", "C", "B", "A"]);
    assert_eq!(
        by(SortField::Artist, true),
        ["C", "D", "A", "B"],
        "ties use the default order"
    );
}

#[test]
fn sort_by_rating_and_duration() {
    let mut s = annotated();
    s.set_rating(&id(&s, "d"), Some(5)).unwrap();
    s.set_rating(&id(&s, "a"), Some(3)).unwrap();
    let f = FilterSpec {
        sort: Some(Sort {
            field: SortField::Rating,
            descending: true,
        }),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["D", "A", "B", "C"]);
    let f = FilterSpec {
        sort: Some(Sort {
            field: SortField::Duration,
            descending: false,
        }),
        ..Default::default()
    };
    assert_eq!(all(&s, &f).len(), 4);
}

#[test]
fn sort_by_added_orders_newest_first_with_stable_ties() {
    let s = annotated();
    s.conn
        .execute(
            "UPDATE tracks SET added_at = 100 WHERE slug LIKE '%-a-%'",
            [],
        )
        .unwrap();
    s.conn
        .execute(
            "UPDATE tracks SET added_at = 300 WHERE slug LIKE '%-c-%'",
            [],
        )
        .unwrap();
    s.conn
        .execute(
            "UPDATE tracks SET added_at = 200 WHERE slug LIKE '%-b-%' OR slug LIKE '%-d-%'",
            [],
        )
        .unwrap();
    let f = FilterSpec {
        sort: Some(Sort {
            field: SortField::Added,
            descending: true,
        }),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["C", "B", "D", "A"]);
}

#[test]
fn limit_caps_total_before_paging() {
    let s = annotated();
    let f = FilterSpec {
        limit: Some(3),
        ..Default::default()
    };
    let p1 = s.search(&f, Page::numbered(1, 2)).unwrap();
    let p2 = s.search(&f, Page::numbered(2, 2)).unwrap();
    let p3 = s.search(&f, Page::numbered(3, 2)).unwrap();
    assert_eq!(
        (p1.total, p1.tracks.len(), p2.tracks.len(), p3.tracks.len()),
        (3, 2, 1, 0)
    );
    assert_eq!(s.search(&f, Page::ALL).unwrap().tracks.len(), 3);
    let f = FilterSpec {
        limit: Some(10),
        ..Default::default()
    };
    assert_eq!(
        s.search(&f, Page::ALL).unwrap().total,
        4,
        "a limit above the matches changes nothing"
    );
    let f = FilterSpec {
        limit: Some(0),
        ..Default::default()
    };
    assert!(s.search(&f, Page::ALL).is_err());
}

#[test]
fn limit_with_sort_keeps_the_top_matches() {
    let mut s = annotated();
    s.set_rating(&id(&s, "a"), Some(5)).unwrap();
    s.set_rating(&id(&s, "d"), Some(4)).unwrap();
    s.set_rating(&id(&s, "c"), Some(1)).unwrap();
    let f = FilterSpec {
        sort: Some(Sort {
            field: SortField::Rating,
            descending: true,
        }),
        limit: Some(2),
        ..Default::default()
    };
    assert_eq!(names(&all(&s, &f)), ["A", "D"]);
}

#[test]
fn manual_playlists_keep_order_and_reject_filters_semantics() {
    let mut s = annotated();
    let (a, b, c, d) = (id(&s, "a"), id(&s, "b"), id(&s, "c"), id(&s, "d"));
    let p = s
        .create_manual_playlist("Road trip", Some("Long drives"))
        .unwrap();
    assert_eq!((p.kind, p.id.as_str()), (PlaylistKind::Manual, "road-trip"));
    assert_eq!(
        s.playlist_add("road-trip", &[d.clone(), a.clone(), c.clone()])
            .unwrap(),
        3
    );
    assert_eq!(
        s.playlist_add("Road Trip", &[a.clone(), b.clone()])
            .unwrap(),
        1,
        "duplicates are skipped"
    );
    let r = s.manual_playlist_tracks("road-trip", Page::ALL).unwrap();
    assert_eq!((r.total, names(&r.tracks)), (4, vec!["D", "A", "C", "B"]));
    let r = s
        .manual_playlist_tracks("road-trip", Page::numbered(2, 3))
        .unwrap();
    assert_eq!((r.total, names(&r.tracks)), (4, vec!["B"]));

    assert_eq!(
        s.playlist_remove("road-trip", &[a.clone(), "nope-nope".into()])
            .unwrap_err()
            .to_string(),
        "not found: track nope-nope"
    );
    assert_eq!(s.playlist_remove("road-trip", &[a]).unwrap(), 1);
    s.playlist_add("road-trip", &[id(&s, "a")]).unwrap();
    let r = s.manual_playlist_tracks("road-trip", Page::ALL).unwrap();
    assert_eq!(
        names(&r.tracks),
        ["D", "C", "B", "A"],
        "re-added tracks go to the end"
    );
}

#[test]
fn dynamic_playlists_reject_membership_edits_and_manual_ones_list_as_manual() {
    let mut s = annotated();
    s.create_playlist("Dyn", None, &FilterSpec::default())
        .unwrap();
    s.create_manual_playlist("Man", None).unwrap();
    let a = id(&s, "a");
    assert!(matches!(
        s.playlist_add("dyn", std::slice::from_ref(&a)),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        s.playlist_remove("dyn", &[a]),
        Err(Error::InvalidInput(_))
    ));
    let kinds: Vec<_> = s
        .playlists()
        .unwrap()
        .into_iter()
        .map(|p| (p.id, p.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("dyn".to_string(), PlaylistKind::Dynamic),
            ("man".to_string(), PlaylistKind::Manual)
        ]
    );
}

#[test]
fn dynamic_playlist_persists_sort_and_limit() {
    let mut s = annotated();
    let f = FilterSpec {
        sort: Some(Sort {
            field: SortField::Bpm,
            descending: true,
        }),
        limit: Some(2),
        favorite: Some(false),
        rating_min: Some(1),
        ..Default::default()
    };
    s.create_playlist("Top", None, &f).unwrap();
    assert_eq!(s.playlist("top").unwrap().filter, f);
}

#[test]
fn tracks_by_slugs_keeps_order_and_duplicates() {
    let s = annotated();
    let (a, b) = (id(&s, "a"), id(&s, "b"));
    let got = s
        .tracks_by_slugs(&[b.clone(), a.clone(), b.clone(), "missing".into()])
        .unwrap();
    assert_eq!(ids_of(&got), [b.as_str(), a.as_str(), b.as_str()]);
}

#[test]
fn session_round_trips_queue_order_and_state() {
    let mut s = annotated();
    let slugs: Vec<String> = ["a", "b", "c", "d"].iter().map(|t| id(&s, t)).collect();
    let mut rng = Rng::new(11);
    let mut q = s.load_queue().unwrap();
    assert!(q.is_empty() && q.state == PlayerState::default());

    q.state.shuffle = true;
    q.state.repeat = RepeatMode::Playlist;
    q.state.end_of_queue = EndOfQueue::Continue;
    q.replace(slugs.iter().map(|t| TrackId(t.clone())).collect(), &mut rng);
    q.append(vec![TrackId(slugs[0].clone())], &mut rng); // duplicates are fine
    q.set_sleep_until(Some(123_456));
    q.jump(2).unwrap();
    s.save_queue(&q).unwrap();

    assert_eq!(s.load_queue().unwrap(), q);

    q.clear();
    s.save_queue(&q).unwrap();
    let back = s.load_queue().unwrap();
    assert_eq!(back, q);
    assert!(back.state.shuffle && back.state.repeat == RepeatMode::Playlist);
}

#[test]
fn saving_a_queue_with_an_unknown_track_fails_without_changing_the_saved_one() {
    let mut s = annotated();
    let a = id(&s, "a");
    let mut rng = Rng::new(1);
    let mut q = Queue::default();
    q.replace(vec![TrackId(a.clone())], &mut rng);
    s.save_queue(&q).unwrap();

    q.append(vec![TrackId("ghost".into())], &mut rng);
    assert!(matches!(s.save_queue(&q), Err(Error::NotFound(_))));
    assert_eq!(s.load_queue().unwrap().items, [TrackId(a)]);
}
