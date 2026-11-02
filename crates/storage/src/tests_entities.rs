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

fn pairs(st: &Storage, kind: EntityKind) -> Vec<(String, String)> {
    st.duplicates(kind, None)
        .unwrap()
        .into_iter()
        .map(|p| (p.a.id, p.b.id))
        .collect()
}

#[test]
fn an_import_always_makes_new_artists_and_the_same_name_is_asked_about() {
    let mut st = library(&[
        song("/1", "a", "Aventura", None),
        song("/2", "b", "Aventura", None),
    ]);
    let mark = st.dedup_mark().unwrap();
    // a later import never attaches to the artist that is already there
    st.import_tracks(&[
        song("/3", "c", "Aventura", None),
        song("/4", "d", "aventura", None),
    ])
    .unwrap();
    assert_eq!(artist_names(&st), ["Aventura", "Aventura"]);
    assert_eq!(st.artist("aventura").unwrap().stats.tracks, 2);
    assert_eq!(
        st.artist("aventura-2").unwrap().stats.tracks,
        2,
        "one new artist per name per run"
    );

    let asked = st.duplicates(EntityKind::Artist, Some(&mark)).unwrap();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].reason, "same name");
    assert_eq!(
        (asked[0].a.id.as_str(), asked[0].b.id.as_str()),
        ("aventura", "aventura-2")
    );
    // nothing new since the import: nothing to ask
    let later = st.dedup_mark().unwrap();
    assert!(
        st.duplicates(EntityKind::Artist, Some(&later))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn similar_names_are_suggested_and_can_be_marked_as_different() {
    let mut st = library(&[
        song("/1", "a", "Daft Punk", None),
        song("/2", "b", "Daft Punk", None),
        song("/3", "c", "Daft-Punk", None),
        song("/4", "d", "Someone Else", None),
    ]);
    let found = st.duplicates(EntityKind::Artist, None).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].a.name.as_str(), found[0].b.name.as_str()),
        ("Daft Punk", "Daft-Punk"),
        "the artist with more tracks comes first"
    );
    st.separate_duplicates(EntityKind::Artist, "daft-punk", "daft-punk-2")
        .unwrap();
    assert!(pairs(&st, EntityKind::Artist).is_empty());
    assert!(
        st.separate_duplicates(EntityKind::Artist, "daft-punk", "daft-punk")
            .is_err()
    );
    assert!(
        st.separate_duplicates(EntityKind::Playlist, "a", "b")
            .is_err()
    );
    assert_eq!(artist_names(&st).len(), 3, "nothing was merged");
}

#[test]
fn merging_artists_moves_everything_and_keeps_no_alias() {
    let mut st = library(&[
        song("/1", "a", "Daft Punk", Some("Discovery")),
        song("/2", "b", "Daft-Punk", Some("Discovery")),
        song("/3", "c", "Daft-Punk", Some("Homework")),
    ]);
    st.add_tags_to(EntityKind::Artist, "daft-punk-2", &s(&["french"]))
        .unwrap();
    let stats = st
        .merge_duplicates(EntityKind::Artist, "daft-punk-2", "daft-punk")
        .unwrap();
    assert_eq!((stats.tracks, stats.albums), (2, 2));

    assert_eq!(artist_names(&st), ["Daft Punk"]);
    let a = st.artist("daft-punk").unwrap();
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
    assert!(st.artist("daft-punk-2").is_err(), "the old ID is gone");
    assert!(
        st.merge_duplicates(EntityKind::Artist, "daft-punk", "daft-punk")
            .is_err()
    );
}

#[test]
fn featured_artists_are_entries_of_their_own_linked_to_artists() {
    let mut st = library(&[
        song(
            "/1",
            "Una Noche",
            "Aventura feat. Romeo Santos",
            Some("Gold"),
        ),
        song("/2", "Otra", "Aventura", Some("Gold")),
        song("/3", "Solo (ft. Kaysha & Ben)", "Romeo Santos", None),
    ]);
    let d = st.track_featured("aventura-gold-una-noche").unwrap();
    assert_eq!(
        d.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["Romeo Santos"]
    );
    // the credit is the same artist as the one with their own track in this run
    let rs = st.artist("romeo-santos").unwrap();
    assert_eq!((rs.stats.tracks, rs.featured_on.len()), (1, 1));
    assert_eq!(rs.featured_on[0].name, "Una Noche");
    let t = st.track("aventura-gold-una-noche").unwrap();
    assert_eq!(
        t.metadata.artist.as_deref(),
        Some("Aventura"),
        "the track keeps its primary artist"
    );
    assert_eq!(st.artist("aventura").unwrap().stats.tracks, 2);
    let solo = st
        .track_featured("romeo-santos-solo-ft-kaysha-and-ben")
        .unwrap();
    assert_eq!(
        solo.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["Kaysha", "Ben"]
    );
    let list = st.artists(&FilterSpec::default()).unwrap();
    assert_eq!(
        list.iter().find(|a| a.name == "Kaysha").unwrap().featured,
        1
    );

    // merging moves the credits and a track never features its own artist
    st.merge_duplicates(EntityKind::Artist, "romeo-santos", "aventura")
        .unwrap();
    assert!(
        st.track_featured("aventura-gold-una-noche")
            .unwrap()
            .is_empty()
    );
    assert_eq!(st.artist("aventura").unwrap().featured_on.len(), 0);
}

#[test]
fn albums_and_tracks_are_checked_for_duplicates_the_same_way() {
    let mut st = library(&[
        song("/a/1.mp3", "Una Noche", "Aventura", Some("Gold")),
        song("/a/2.mp3", "Otra", "Aventura", Some("Gold")),
    ]);
    // a second copy of the same music, imported from elsewhere
    st.import_tracks(&[
        song("/b/1.mp3", "Una noche!", "Aventura", Some("Gold")),
        song("/b/2.mp3", "Otra", "Aventura", Some("Gold")),
    ])
    .unwrap();
    let albums = st.duplicates(EntityKind::Album, None).unwrap();
    assert_eq!(albums.len(), 1, "{albums:?}");
    assert_eq!(albums[0].a.name, "Gold");
    let tracks = st.duplicates(EntityKind::Track, None).unwrap();
    assert_eq!(tracks.len(), 2, "{tracks:?}");
    // a track of a very different length is not the same recording
    let mut long = song("/c/1.mp3", "Una Noche", "Aventura", None);
    long.metadata.duration_ms = Some(400_000);
    st.import_tracks(&[long]).unwrap();
    assert_eq!(st.duplicates(EntityKind::Track, None).unwrap().len(), 2);

    // merge the duplicate album, then a duplicate track keeps its files, tags and place
    st.merge_duplicates(EntityKind::Album, "aventura-gold-2", "aventura-gold")
        .unwrap();
    assert_eq!(st.album("aventura-gold").unwrap().tracks.len(), 4);
    let (keep, drop) = ("aventura-gold-una-noche", "aventura-gold-una-noche-2");
    st.add_tags(drop, &s(&["practice"])).unwrap();
    st.set_rating(drop, Some(5)).unwrap();
    st.create_manual_playlist("Mix", None).unwrap();
    st.playlist_add("mix", &[drop.into(), "aventura-gold-otra".into()])
        .unwrap();
    let stats = st.merge_duplicates(EntityKind::Track, drop, keep).unwrap();
    assert_eq!(stats.tracks, 1);
    assert!(st.track(drop).is_err());
    let kept = st.track(keep).unwrap();
    assert_eq!(
        (kept.rating, kept.tags.contains(&"practice".to_string())),
        (Some(5), true)
    );
    assert_eq!(st.source_paths(keep).unwrap(), ["/a/1.mp3", "/b/1.mp3"]);
    assert_eq!(st.source_path(keep).unwrap().as_deref(), Some("/a/1.mp3"));
    let in_mix: Vec<String> = st
        .conn
        .prepare(
            "SELECT t.slug FROM playlist_tracks p JOIN tracks t ON t.id = p.track_id
             WHERE p.playlist_id = 'mix' ORDER BY p.play_order",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert_eq!(in_mix, [keep, "aventura-gold-otra"]);
    // a re-import of the same files adds nothing
    let again = st
        .import_tracks(&[song("/b/1.mp3", "Una noche!", "Aventura", None)])
        .unwrap();
    assert_eq!(again.added, 0);
}

#[test]
fn splitting_an_artist_makes_a_new_one_that_can_share_the_name() {
    let mut st = library(&[
        song("/1", "a", "John Smith", Some("Blues")),
        song("/2", "b", "John Smith", Some("Blues")),
        song("/3", "c", "John Smith", Some("Jazz")),
        song("/4", "d", "John Smith", None),
    ]);
    let new = st
        .split_artist(
            "john-smith",
            &s(&["john-smith-d"]),
            &s(&["john-smith-jazz"]),
            None,
            Some("John Smith (pianist)"),
        )
        .unwrap();
    assert_eq!(
        (new.id.as_str(), new.name.as_str()),
        ("john-smith-pianist", "John Smith")
    );
    assert_eq!(new.stats.tracks, 2);
    assert_eq!(st.artist("john-smith").unwrap().stats.tracks, 2);
    assert_eq!(
        st.album("john-smith-jazz").unwrap().artist.unwrap().id,
        "john-smith-pianist"
    );
    assert!(
        pairs(&st, EntityKind::Artist).is_empty(),
        "a split marks them as different people"
    );
    // errors change nothing
    assert!(st.split_artist("john-smith", &[], &[], None, None).is_err());
    assert!(
        st.split_artist("john-smith", &s(&["john-smith-d"]), &[], None, None)
            .is_err(),
        "not theirs any more"
    );
    assert!(
        st.split_artist(
            "john-smith",
            &[],
            &s(&["john-smith-blues"]),
            None,
            Some("john-smith-pianist")
        )
        .is_err()
    );
    assert_eq!(artist_names(&st), ["John Smith", "John Smith"]);
    // a rename of the ID
    let renamed = st
        .set_artist_slug("john-smith-pianist", "J. Smith (piano)")
        .unwrap();
    assert_eq!(renamed.id, "j-smith-piano");
    assert!(st.artist("john-smith-pianist").is_err());
    assert!(
        st.set_artist_slug("j-smith-piano", "john-smith").is_err(),
        "taken"
    );
    assert!(st.set_artist_slug("j-smith-piano", "???").is_err());
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
