//! Artists, albums, their profiles, tags on every kind of thing, and manual deduplication.

use super::*;
use weave_core::{ChildPolicy, Count, EntityKind, TagOp};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn song(path: &str, title: &str, artist: &str, album: Option<&str>) -> ScannedTrack {
    ScannedTrack {
        path: path.into(),
        metadata: Metadata {
            title: title.into(),
            artist: Some(artist.into()),
            album: album.map(Into::into),
            genre: Some("Bachata".into()),
            year: Some(2000),
            bpm: Some(100.0),
            duration_ms: Some(180_000),
            track_number: Some(1),
            ..Default::default()
        },
    }
}

fn library(tracks: &[ScannedTrack]) -> Storage {
    let mut st = Storage::open_in_memory().unwrap();
    st.import_tracks(tracks).unwrap();
    st
}

fn artist_names(st: &Storage) -> Vec<String> {
    st.artists(&FilterSpec::default())
        .unwrap()
        .into_iter()
        .map(|a| a.name)
        .collect()
}

#[test]
fn the_same_spelling_is_one_artist_but_similar_names_stay_apart() {
    let st = library(&[
        song("/1", "a", "Daft Punk", Some("Discovery")),
        song("/2", "b", "daft   PUNK", Some("Discovery")),
        song("/3", "c", "Daft-Punk", None),
        song("/4", "d", "The Daft Punk", None),
    ]);
    assert_eq!(
        artist_names(&st),
        ["Daft Punk", "Daft-Punk", "The Daft Punk"],
        "case and spacing group; punctuation and a leading \"the\" do not"
    );
    let a = st.artist("daft-punk").unwrap();
    assert_eq!((a.stats.tracks, a.albums.len(), a.loose_tracks), (2, 1, 0));
    assert_eq!(a.albums[0].id, "daft-punk-discovery");
}

#[test]
fn similar_names_are_suggested_and_can_be_marked_as_different_people() {
    let mut st = library(&[
        song("/1", "a", "Daft Punk", None),
        song("/2", "b", "Daft Punk", None),
        song("/3", "c", "Daft-Punk", None),
        song("/4", "d", "Someone Else", None),
    ]);
    let pairs = st.similar_artists().unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        (pairs[0].a.name.as_str(), pairs[0].b.name.as_str()),
        ("Daft Punk", "Daft-Punk"),
        "the artist with more tracks comes first"
    );
    st.separate_artists("daft-punk", "daft-punk-2").unwrap();
    assert!(st.similar_artists().unwrap().is_empty());
    assert!(st.separate_artists("daft-punk", "daft-punk").is_err());
    assert_eq!(artist_names(&st).len(), 3, "nothing was merged");
}

#[test]
fn merging_moves_everything_and_keeps_the_old_spelling_as_an_alias() {
    let mut st = library(&[
        song("/1", "a", "Daft Punk", Some("Discovery")),
        song("/2", "b", "Daft-Punk", Some("Discovery")),
        song("/3", "c", "Daft-Punk", Some("Homework")),
    ]);
    st.add_tags_to(EntityKind::Artist, "daft-punk-2", &s(&["french"]))
        .unwrap();
    let stats = st.merge_artists("daft-punk-2", "daft-punk").unwrap();
    assert_eq!((stats.tracks, stats.albums), (2, 2));

    assert_eq!(artist_names(&st), ["Daft Punk"]);
    let a = st.artist("daft-punk").unwrap();
    assert_eq!(a.aliases, ["Daft-Punk"]);
    assert_eq!(a.stats.tracks, 3);
    assert_eq!(
        a.albums
            .iter()
            .map(|x| (x.title.as_str(), x.tracks))
            .collect::<Vec<_>>(),
        [("Discovery", 2), ("Homework", 1)],
        "the two Discovery albums became one"
    );
    assert_eq!(a.tags, ["french"], "tags move over");
    for t in st.search(&FilterSpec::default(), Page::ALL).unwrap().tracks {
        assert_eq!(t.metadata.artist.as_deref(), Some("Daft Punk"));
    }
    // the old name still finds the survivor, and a later import lands on it
    assert_eq!(st.artist("Daft-Punk").unwrap().id, "daft-punk");
    st.import_tracks(&[song("/4", "d", "daft-punk", None)])
        .unwrap();
    assert_eq!(artist_names(&st), ["Daft Punk"]);
    assert_eq!(st.artist("daft-punk").unwrap().stats.tracks, 4);
    assert!(st.merge_artists("daft-punk", "daft-punk").is_err());
}

#[test]
fn albums_link_to_the_album_artist_and_can_be_relinked() {
    let mut st = Storage::open_in_memory().unwrap();
    let mut t = song("/1", "a", "Guest Singer", Some("Compilation"));
    t.metadata.album_artist = Some("Various Artists".into());
    st.import_tracks(&[t, song("/2", "b", "Guest Singer", Some("Solo"))])
        .unwrap();
    let comp = st
        .album("compilation")
        .unwrap_or_else(|_| st.album("various-artists-compilation").unwrap());
    assert_eq!(comp.artist.as_ref().unwrap().name, "Various Artists");
    let solo = st.album("guest-singer-solo").unwrap();
    assert_eq!(solo.artist.as_ref().unwrap().id, "guest-singer");

    let linked = st
        .link_album("guest-singer-solo", Some("various-artists"))
        .unwrap();
    assert_eq!(linked.artist.unwrap().id, "various-artists");
    let unlinked = st.link_album("guest-singer-solo", None).unwrap();
    assert!(unlinked.artist.is_none());
    assert!(st.link_album("nope", None).is_err());
    assert!(st.link_album("guest-singer-solo", Some("nobody")).is_err());
}

#[test]
fn profiles_summarize_what_the_library_holds() {
    let mut st = library(&[
        song("/1", "a", "Aventura", Some("Gold")),
        song("/2", "b", "Aventura", Some("Gold")),
        song("/3", "c", "Aventura", None),
    ]);
    let a = st.track("aventura-gold-a").unwrap().id.0;
    st.add_tags(&a, &s(&["practice"])).unwrap();
    st.set_rating(&a, Some(4)).unwrap();
    let p = st.artist("aventura").unwrap();
    assert_eq!((p.stats.tracks, p.stats.duration_ms), (3, 540_000));
    assert_eq!(
        (p.stats.year_min, p.stats.year_max),
        (Some(2000), Some(2000))
    );
    assert_eq!(
        (p.stats.bpm_min, p.stats.bpm_avg),
        (Some(100.0), Some(100.0))
    );
    assert_eq!((p.stats.rated, p.stats.rating_avg), (1, Some(4.0)));
    assert_eq!(p.loose_tracks, 1);
    assert_eq!(
        p.genres,
        [Count {
            name: "genre:Bachata".into(),
            count: 3
        }]
    );
    assert_eq!(
        p.track_tags,
        [Count {
            name: "practice".into(),
            count: 1
        }]
    );
    let album = st.album("aventura-gold").unwrap();
    assert_eq!(album.tracks.len(), 2);
    assert_eq!(album.stats.tracks, 2);
    assert!(st.artist("nobody").is_err());
}

#[test]
fn tags_go_on_artists_albums_and_playlists_and_are_found_through_the_tree() {
    let mut st = library(&[
        song("/1", "a", "Aventura", Some("Gold")),
        song("/2", "b", "Kaysha", Some("Zouk Vol 1")),
    ]);
    st.create_tag("genre:latin", None).unwrap();
    st.create_tag("genre:bachata-roots", Some("genre:latin"))
        .unwrap();
    st.add_tags_to(
        EntityKind::Artist,
        "aventura",
        &s(&["genre:bachata-roots", "legend"]),
    )
    .unwrap();
    st.add_tags_to(EntityKind::Album, "kaysha-zouk-vol-1", &s(&["genre:latin"]))
        .unwrap();
    st.create_manual_playlist("Road", None).unwrap();
    st.add_tags_to(EntityKind::Playlist, "road", &s(&["trip"]))
        .unwrap();

    assert_eq!(
        st.tags_of(EntityKind::Artist, "aventura").unwrap(),
        ["genre:bachata-roots", "legend"]
    );
    assert_eq!(st.tags_of(EntityKind::Playlist, "road").unwrap(), ["trip"]);
    assert!(
        st.add_tags_to(EntityKind::Artist, "nobody", &s(&["x"]))
            .is_err()
    );
    assert!(
        st.add_tags_to(EntityKind::Album, "gold", &s(&["bpm:fast"]))
            .is_err()
    );

    // the parent reaches the child on artists, exact does not, exclude removes both
    let by = |f: FilterSpec| -> Vec<String> {
        st.artists(&f)
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect()
    };
    let spec = |all: &[&str], excl: &[&str]| FilterSpec {
        tag_all: s(all),
        tag_exclude: s(excl),
        ..Default::default()
    };
    assert_eq!(by(spec(&["genre:latin"], &[])), ["Aventura"]);
    assert!(by(spec(&["=genre:latin"], &[])).is_empty());
    assert_eq!(by(spec(&[], &["genre:latin"])), ["Kaysha"]);
    let clause = FilterSpec {
        tag_clauses: vec![weave_core::TagClause {
            op: TagOp::Any,
            keys: s(&["genre"]),
            values: s(&["latin"]),
            exact: false,
        }],
        ..Default::default()
    };
    assert_eq!(st.albums(&clause, None).unwrap().len(), 1);
    assert_eq!(st.albums(&clause, None).unwrap()[0].tags, ["genre:latin"]);

    // tags are counted per kind, and deleting one removes it everywhere
    let latin = st.tag("genre:latin").unwrap();
    assert_eq!(
        (latin.tracks, latin.artists, latin.albums, latin.playlists),
        (0, 0, 1, 0)
    );
    st.delete_tag("genre:latin", ChildPolicy::Delete).unwrap();
    assert_eq!(
        st.tags_of(EntityKind::Artist, "aventura").unwrap(),
        ["legend"]
    );
    assert!(
        st.tags_of(EntityKind::Album, "kaysha-zouk-vol-1")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_track_knows_its_artist_and_album() {
    let st = library(&[
        song("/1", "a", "Aventura", Some("Gold")),
        song("/2", "b", "x", None),
    ]);
    let (added, artist, album) = st.track_links("aventura-gold-a").unwrap();
    assert!(added > 0);
    assert_eq!(artist.unwrap().id, "aventura");
    assert_eq!(album.unwrap().id, "aventura-gold");
    let (_, artist, album) = st.track_links("x-b").unwrap();
    assert!(artist.is_some() && album.is_none());
}
