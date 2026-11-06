//! Complete AND/OR groups across tags, metadata and playlist sources.
use super::*;
use weave_player_core::{FilterJoin, Sort, SortField};

fn populate(storage: &mut Storage) {
    for (title, mood, bpm) in [
        ("romantic slow", "romantic", Some(80.0)),
        ("romantic fast", "romantic", Some(140.0)),
        ("party slow", "party", Some(80.0)),
        ("party fast", "party", Some(140.0)),
        ("romantic unknown", "romantic", None),
        ("neither", "other", Some(80.0)),
    ] {
        storage
            .import_tracks(&[ScannedTrack {
                path: format!("/{title}"),
                metadata: Metadata {
                    title: title.into(),
                    artist: Some("x".into()),
                    bpm,
                    ..Default::default()
                },
            }])
            .unwrap();
        storage
            .add_tags(
                &format!("x-{}", title.replace(' ', "-")),
                &[format!("mood:{mood}")],
            )
            .unwrap();
    }
}
fn grouped() -> FilterSpec {
    FilterSpec {
        group_join: FilterJoin::Or,
        groups: vec![
            FilterSpec {
                tag_all: vec!["mood:romantic".into()],
                bpm_max: Some(90.0),
                ..Default::default()
            },
            FilterSpec {
                tag_all: vec!["mood:party".into()],
                bpm_min: Some(120.0),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
fn titles(storage: &Storage, filter: &FilterSpec) -> Vec<String> {
    storage
        .search(filter, Page::ALL)
        .unwrap()
        .tracks
        .into_iter()
        .map(|track| track.metadata.title)
        .collect()
}
#[test]
fn or_combines_complete_and_groups_with_global_constraints_and_result_paging() {
    let mut storage = Storage::open_in_memory().unwrap();
    populate(&mut storage);
    let filter = grouped();
    assert_eq!(titles(&storage, &filter), ["party fast", "romantic slow"]);
    let mut and = filter.clone();
    and.group_join = FilterJoin::And;
    assert!(titles(&storage, &and).is_empty());
    let mut outer = filter.clone();
    outer.query = Some("slow".into());
    assert_eq!(titles(&storage, &outer), ["romantic slow"]);
    let mut nested = FilterSpec {
        groups: vec![
            filter.clone(),
            FilterSpec {
                query: Some("slow".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(titles(&storage, &nested), ["romantic slow"]);
    nested.groups.push(FilterSpec::default());
    assert_eq!(
        titles(&storage, &nested),
        ["romantic slow"],
        "empty drafts are ignored"
    );
    let limited = FilterSpec {
        sort: Some(Sort {
            field: SortField::Bpm,
            descending: true,
        }),
        limit: Some(1),
        ..filter
    };
    let page = storage.search(&limited, Page::ALL).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.tracks[0].metadata.title, "party fast");
    assert!(
        storage
            .search(&limited, Page::numbered(2, 1))
            .unwrap()
            .tracks
            .is_empty()
    );
}
#[test]
fn grouped_tag_predicates_apply_to_entity_tags_without_track_tag_inheritance() {
    use weave_player_core::EntityKind;
    let mut storage = Storage::open_in_memory().unwrap();
    populate(&mut storage);
    storage
        .add_tags_to(EntityKind::Artist, "x", &["legend".into(), "french".into()])
        .unwrap();
    let mut filter = FilterSpec {
        group_join: FilterJoin::Or,
        groups: vec![
            FilterSpec {
                tag_all: vec!["legend".into(), "french".into()],
                ..Default::default()
            },
            FilterSpec {
                tag_all: vec!["romantic".into(), "spanish".into()],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(storage.artists(&filter).unwrap().len(), 1);
    assert!(
        titles(&storage, &filter).is_empty(),
        "artist tags never become track tags"
    );
    filter.group_join = FilterJoin::And;
    assert!(storage.artists(&filter).unwrap().is_empty());
}

#[test]
fn nested_group_sources_are_canonicalized_checked_for_cycles_and_detached() {
    let mut storage = Storage::open_in_memory().unwrap();
    populate(&mut storage);
    storage.create_manual_playlist("My picks", None).unwrap();
    storage
        .playlist_add("My picks", &["x-romantic-slow".into()])
        .unwrap();
    let mut filter = grouped();
    filter.groups[1].sources = vec!["MY PICKS".into()];
    let saved = storage.create_playlist("Grouped", None, &filter).unwrap();
    assert_eq!(saved.filter.groups[1].sources, ["my-picks"]);
    assert_eq!(
        titles(
            &storage,
            &FilterSpec {
                sources: vec!["Grouped".into()],
                ..Default::default()
            }
        ),
        ["romantic slow"]
    );
    assert_eq!(
        storage.playlist_dependents("My picks").unwrap()[0]
            .playlist
            .name,
        "Grouped"
    );
    let cycle = FilterSpec {
        groups: vec![FilterSpec {
            sources: vec!["Grouped".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(storage.update_playlist("my-picks", None, &cycle).is_err());
    assert!(storage.delete_playlist("My picks", false).is_err());
    storage.delete_playlist("My picks", true).unwrap();
    let saved = storage.playlist("Grouped").unwrap();
    assert!(saved.filter.all_sources().is_empty());
    assert_eq!(
        titles(&storage, &saved.filter),
        ["party fast", "romantic slow"]
    );
}
#[test]
fn saved_groups_roundtrip_on_disk_and_manual_rules_only_offer_candidates() {
    let directory = std::env::temp_dir().join(format!(
        "weave-filter-groups-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let path = directory.join("library.db");
    let mut storage = Storage::open(&path).unwrap();
    populate(&mut storage);
    storage
        .create_playlist("Grouped", None, &grouped())
        .unwrap();
    let manual = storage
        .create_manual_playlist_with_filter("Curated", None, &grouped())
        .unwrap();
    assert_eq!(
        titles(&storage, &manual.filter),
        ["party fast", "romantic slow"]
    );
    assert_eq!(
        storage
            .manual_playlist_tracks(&manual.id, Page::ALL)
            .unwrap()
            .total,
        0
    );
    storage
        .playlist_add("Curated", &["x-neither".into()])
        .unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.playlist("Grouped").unwrap().filter, grouped());
    assert_eq!(
        titles(&storage, &storage.playlist("Grouped").unwrap().filter),
        ["party fast", "romantic slow"]
    );
    assert_eq!(
        storage
            .manual_playlist_tracks(&manual.id, Page::ALL)
            .unwrap()
            .tracks[0]
            .metadata
            .title,
        "neither"
    );
    let legacy: FilterSpec = serde_json::from_str(r#"{"tag_all":["mood:romantic"]}"#).unwrap();
    assert!(legacy.groups.is_empty());
    assert_eq!(legacy.group_join, FilterJoin::And);
    drop(storage);
    std::fs::remove_dir_all(directory).unwrap();
}
