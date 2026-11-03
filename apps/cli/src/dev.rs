//! Debug-build-only helpers. Compiled out of release builds.

use weave_player_lib::Library;
use weave_player_lib::core::{
    EntityKind, FilterSpec, Metadata, Result, ScannedTrack, Sort, SortField, TagClause, TagOp,
    parse_credits, track_slug,
};

pub const MAX_COUNT: usize = 100;

struct Artist {
    name: &'static str,
    lang: &'static str,
    genre: &'static str,
    bpm: (u32, u32),
    years: (i32, i32),
}

const ARTISTS: [Artist; 8] = [
    Artist {
        name: "Romeo Santos",
        lang: "spanish",
        genre: "Bachata",
        bpm: (95, 135),
        years: (2011, 2022),
    },
    Artist {
        name: "Aventura",
        lang: "spanish",
        genre: "Bachata",
        bpm: (90, 130),
        years: (1999, 2009),
    },
    Artist {
        name: "Kaysha",
        lang: "french",
        genre: "Zouk",
        bpm: (80, 110),
        years: (2006, 2014),
    },
    Artist {
        name: "Édith Piaf",
        lang: "french",
        genre: "Chanson",
        bpm: (70, 120),
        years: (1947, 1962),
    },
    Artist {
        name: "Daft Punk",
        lang: "english",
        genre: "Electronic",
        bpm: (110, 130),
        years: (1997, 2013),
    },
    Artist {
        name: "Nina Simone",
        lang: "english",
        genre: "Jazz",
        bpm: (60, 110),
        years: (1958, 1974),
    },
    Artist {
        name: "Buena Vista Social Club",
        lang: "spanish",
        genre: "Son",
        bpm: (85, 125),
        years: (1997, 2000),
    },
    Artist {
        name: "Rock & Roll Trio",
        lang: "english",
        genre: "Rock",
        bpm: (120, 170),
        years: (1976, 1989),
    },
];
const ALBUMS: [&str; 3] = ["Primera Parte", "Don't Look Back", "Live at the Roxy"];
const ADJECTIVES: [&str; 10] = [
    "Blue", "Midnight", "Golden", "Silent", "Burning", "Lazy", "Electric", "Broken", "Sweet",
    "Wild",
];
const NOUNS: [&str; 10] = [
    "Heart", "Rain", "Dance", "River", "Summer", "Shadow", "Love", "City", "Dream", "Fire",
];

/// The metadata of fake track `i`.
fn metadata(i: usize) -> Metadata {
    let a = &ARTISTS[i % ARTISTS.len()];
    let span = |lo: u32, hi: u32| lo + (i as u32 * 37) % (hi - lo + 1);
    let title = format!("{} {}", ADJECTIVES[i % 10], NOUNS[i / 10 % 10]);
    // Some of Daft Punk's tracks spell the name with a hyphen: a look-alike artist that is
    // never merged by itself, to try `dedup`.
    let spelled = if a.name == "Daft Punk" && i % 3 == 1 {
        "Daft-Punk"
    } else {
        a.name
    };
    // Some tracks feature another artist: a credit of its own, linked to that artist.
    let credited = if i % 17 == 5 {
        format!("{spelled} feat. {}", ARTISTS[(i + 3) % ARTISTS.len()].name)
    } else {
        spelled.to_string()
    };
    Metadata {
        title,
        artist: (i % 13 != 12).then_some(credited),
        album: (i % 5 != 4).then(|| ALBUMS[i / ARTISTS.len() % ALBUMS.len()].to_string()),
        album_artist: (i % 5 != 4).then(|| a.name.to_string()),
        genre: (i % 11 != 10).then(|| a.genre.to_string()),
        composer: i.is_multiple_of(4).then(|| "A. Composer".to_string()),
        year: Some(span(a.years.0 as u32, a.years.1 as u32) as i32),
        track_number: Some((i / 24 + 1) as u32),
        disc_number: Some(1),
        duration_ms: Some(span(150, 330) as u64 * 1000),
        bpm: (i % 7 != 6).then(|| span(a.bpm.0, a.bpm.1) as f64),
    }
}

/// Deterministic fake tracks: unique per (artist, album, title), with a sprinkle of missing
/// fields so empty-value paths get exercised. Every 30th is a second copy (another file) of a
/// track from 20 earlier, to try the duplicate check on tracks.
pub fn tracks(count: usize) -> Vec<ScannedTrack> {
    (0..count.min(MAX_COUNT))
        .map(|i| ScannedTrack {
            path: format!("/weave-seed/{i:03}.mp3"), // no file is behind it
            metadata: metadata(if i % 30 == 29 && i >= 20 { i - 20 } else { i }),
        })
        .collect()
}

/// The slug a seeded track gets: the featured credit is not part of its artist.
fn slug_of(t: &ScannedTrack) -> String {
    let m = &t.metadata;
    let primary = parse_credits(m.artist.as_deref(), &m.title).primary;
    track_slug(primary.as_deref(), m.album.as_deref(), &m.title)
}

/// The seeded tag tree: `(tag, parent)`, parents first. Import already created the plain
/// `genre:` tags, so those are only given a parent; the rest are created here.
const TREE: &[(&str, Option<&str>)] = &[
    ("lang:romance", None),
    ("lang:spanish", Some("lang:romance")),
    ("lang:french", Some("lang:romance")),
    ("lang:germanic", None),
    ("lang:english", Some("lang:germanic")),
    ("genre:latin", None),
    ("genre:bachata", Some("genre:latin")),
    ("genre:bachata-traditional", Some("genre:bachata")),
    ("genre:bachata-sensual", Some("genre:bachata")),
    ("genre:zouk", Some("genre:latin")),
    ("genre:son", Some("genre:latin")),
    ("period:20th-century", None),
    ("period:40s", Some("period:20th-century")),
    ("period:50s", Some("period:20th-century")),
    ("period:60s", Some("period:20th-century")),
    ("period:70s", Some("period:20th-century")),
    ("period:80s", Some("period:20th-century")),
    ("period:90s", Some("period:20th-century")),
    ("period:21st-century", None),
    ("period:00s", Some("period:21st-century")),
    ("period:10s", Some("period:21st-century")),
    ("period:20s", Some("period:21st-century")),
];

/// Create the tree, or complete it: a tag that already has a parent is left alone, so a
/// re-run never undoes a reparenting done by hand.
fn ensure_tree(lib: &mut Library) -> Result<()> {
    for (name, parent) in TREE {
        match lib.tag(name) {
            Ok(tag) if tag.parent.is_none() && parent.is_some() => {
                lib.set_tag_parent(name, *parent)?;
            }
            Ok(_) => {}
            Err(weave_player_lib::core::Error::NotFound(_)) => {
                lib.create_tag(name, *parent)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Tags assigned on top of the genre tag that import derives: the language sung, a subgenre, the
/// era and the mood. Each track gets leaf
/// tags only: a sensual bachata track is not also given `genre:latin`, the tree reaches it.
fn tags_for(t: &ScannedTrack, i: usize) -> Vec<String> {
    let m = &t.metadata;
    // a second copy of a track has the artist of the one it copies
    let base = if i % 30 == 29 && i >= 20 { i - 20 } else { i };
    let mut tags: Vec<String> = vec![format!("lang:{}", ARTISTS[base % ARTISTS.len()].lang)];
    if m.genre.as_deref() == Some("Bachata") {
        let sub = if i.is_multiple_of(2) {
            "traditional"
        } else {
            "sensual"
        };
        tags.push(format!("genre:bachata-{sub}"));
    }
    if let Some(y) = m.year {
        tags.push(format!("period:{:02}s", y % 100 / 10 * 10));
    }
    tags.push(["mood:romantic", "mood:party", "mood:melodic"][i % 3].into());
    if i.is_multiple_of(9) {
        tags.push("live".into());
    }
    if i.is_multiple_of(14) {
        tags.push("remix".into());
    }
    if i % 10 == 3 {
        tags.push("practice".into());
    }
    tags
}

/// Tags on artists and albums (leaf-free: the tree reaches them), and on a playlist.
fn tag_entities(lib: &mut Library) -> Result<usize> {
    let strs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut added = 0;
    for (artist, tags) in [
        ("Romeo Santos", &["genre:bachata", "legend"][..]),
        ("Aventura", &["genre:bachata", "legend", "bronx"]),
        ("Kaysha", &["genre:zouk"]),
        ("Buena Vista Social Club", &["genre:son", "cuba"]),
        ("Nina Simone", &["genre:jazz", "legend"]),
        ("Édith Piaf", &["genre:chanson", "legend"]),
    ] {
        added += lib.add_tags_to(EntityKind::Artist, artist, &strs(tags))?;
    }
    for album in lib.albums(&FilterSpec::default(), None)? {
        if album.title == "Live at the Roxy" {
            added += lib.add_tags_to(EntityKind::Album, &album.id, &strs(&["live"]))?;
        }
    }
    Ok(added)
}

pub struct Seeded {
    pub added: usize,
    pub skipped: usize,
    pub tagged: usize,
    pub playlists: usize,
}

/// Idempotent: re-running adds nothing that already exists.
pub fn seed(lib: &mut Library, count: usize) -> Result<Seeded> {
    let fake = tracks(count);
    let stats = lib.import_scanned(&fake)?;
    ensure_tree(lib)?;

    let mut tagged = 0;
    for (i, t) in fake.iter().enumerate() {
        let slug = slug_of(t);
        tagged += lib.add_tags(&slug, &tags_for(t, i))?;
        // Ratings are only seeded onto tracks that have none yet, so re-running never
        // overwrites something changed by hand.
        if !i.is_multiple_of(3) && lib.track(&slug)?.rating.is_none() {
            lib.set_rating(&slug, Some(1 + (i * 7 % 5) as u8))?;
        }
    }

    tagged += tag_entities(lib)?;

    let created = |r: weave_player_lib::core::Result<weave_player_lib::core::Playlist>| match r {
        Ok(_) => Ok(1),
        Err(weave_player_lib::core::Error::AlreadyExists(_)) => Ok(0),
        Err(e) => Err(e),
    };
    let strs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let dynamic = [
        (
            "Bachata 90s",
            "Slow dance nights",
            FilterSpec {
                tag_all: strs(&["genre:bachata", "period:90s"]),
                tag_exclude: strs(&["live", "remix"]),
                bpm_min: Some(90.0),
                ..Default::default()
            },
        ),
        (
            "Party starters",
            "Fast and fun",
            FilterSpec {
                tag_clauses: vec![TagClause {
                    op: TagOp::Any,
                    keys: strs(&["mood", "genre"]),
                    values: strs(&["party", "electronic"]),
                    exact: false,
                }],
                bpm_min: Some(115.0),
                bpm_max: Some(140.0),
                sort: Some(Sort {
                    field: SortField::Bpm,
                    descending: true,
                }),
                ..Default::default()
            },
        ),
        (
            "Slow Spanish nights",
            "Romantic, sung in Spanish, under 110 BPM",
            FilterSpec {
                tag_all: strs(&["lang:spanish", "mood:romantic"]),
                bpm_max: Some(110.0),
                sort: Some(Sort {
                    field: SortField::Bpm,
                    descending: false,
                }),
                ..Default::default()
            },
        ),
        (
            "Romance languages",
            "Spanish and French, through the language tree",
            FilterSpec {
                tag_any: strs(&["lang:romance"]),
                tag_exclude: strs(&["mood:melodic"]),
                ..Default::default()
            },
        ),
        (
            "Latin nights",
            "Every Latin subgenre, through the tag tree",
            FilterSpec {
                tag_any: strs(&["genre:latin"]),
                sort: Some(Sort {
                    field: SortField::Bpm,
                    descending: false,
                }),
                ..Default::default()
            },
        ),
        (
            "Sensual only",
            "Just the sensual subgenre, no parents",
            FilterSpec {
                tag_all: strs(&["=genre:bachata-sensual"]),
                ..Default::default()
            },
        ),
        (
            "Old classics",
            "Before 1970",
            FilterSpec {
                year_max: Some(1969),
                ..Default::default()
            },
        ),
        (
            "Top rated",
            "Best rated, top 15",
            FilterSpec {
                rating_min: Some(4),
                sort: Some(Sort {
                    field: SortField::Rating,
                    descending: true,
                }),
                limit: Some(15),
                ..Default::default()
            },
        ),
    ];
    let mut playlists = 0;
    for (name, description, filter) in dynamic {
        playlists += created(lib.create_playlist(name, Some(description), filter))?;
    }

    // A manual playlist with the first few seeded tracks, in a hand-picked (reverse) order.
    let made = created(lib.create_manual_playlist("Road trip", Some("Hand-picked, in order")))?;
    lib.add_tags_to(EntityKind::Playlist, "road-trip", &["trip".to_string()])?;
    if made == 1 {
        let picks: Vec<String> = fake.iter().take(8).rev().map(slug_of).collect();
        lib.playlist_add("road-trip", &picks)?;
    }
    playlists += made;

    // Playlists that draw on other playlists: a manual one limits to its tracks, a dynamic one
    // contributes what it matches, several are ORed, and the filter applies on top.
    let from_playlists = [
        (
            "Spanish road trip",
            "The road trip, only what is sung in Spanish",
            FilterSpec {
                sources: strs(&["road-trip"]),
                tag_all: strs(&["lang:spanish"]),
                ..Default::default()
            },
        ),
        (
            "Slow nights or road trip",
            "Either playlist, nothing above 120 BPM",
            FilterSpec {
                sources: strs(&["slow-spanish-nights", "road-trip"]),
                bpm_max: Some(120.0),
                ..Default::default()
            },
        ),
    ];
    for (name, description, filter) in from_playlists {
        playlists += created(lib.create_playlist(name, Some(description), filter))?;
    }

    Ok(Seeded {
        added: stats.added,
        skipped: stats.skipped,
        tagged,
        playlists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_player_lib::core::Page;

    #[test]
    fn seed_is_idempotent_and_populates_everything() {
        let mut lib = Library::open_in_memory().unwrap();
        let first = seed(&mut lib, 60).unwrap();
        assert_eq!((first.added, first.skipped, first.playlists), (60, 0, 11));
        assert!(first.tagged > 60);

        let second = seed(&mut lib, 60).unwrap();
        assert_eq!(
            (
                second.added,
                second.skipped,
                second.tagged,
                second.playlists
            ),
            (0, 60, 0, 0)
        );

        assert_eq!(
            lib.search(&FilterSpec::default(), Page::ALL).unwrap().total,
            60
        );
        let tracks = lib
            .search(&FilterSpec::default(), Page::ALL)
            .unwrap()
            .tracks;
        assert!(tracks.iter().any(|t| t.rating.is_some()));
        // the tree: leaves are assigned, the parents reach them
        let tag_all = |t: &str| FilterSpec {
            tag_all: vec![t.into()],
            ..Default::default()
        };
        let total = |f: &FilterSpec| lib.search(f, Page::ALL).unwrap().total;
        let bachata = total(&tag_all("genre:bachata"));
        assert!(bachata > 0 && total(&tag_all("genre:latin")) >= bachata);
        assert_eq!(
            total(&tag_all("=genre:latin")),
            0,
            "nothing is assigned the root"
        );
        assert!(
            total(&tag_all("=genre:bachata-sensual")) < bachata,
            "a subgenre is narrower than its parent"
        );
        let parents: Vec<_> = lib
            .list_tags()
            .unwrap()
            .into_iter()
            .filter_map(|t| t.parent.map(|p| (t.name, p)))
            .collect();
        assert!(parents.contains(&("genre:Bachata".into(), "genre:latin".into())));
        assert!(parents.contains(&("period:90s".into(), "period:20th-century".into())));
        // a near-duplicate is suggested but never merged by itself; artists and albums carry tags
        let names: Vec<_> = lib
            .artists(&FilterSpec::default())
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert!(
            names.contains(&"Daft Punk".to_string()) && names.contains(&"Daft-Punk".to_string())
        );
        let similar = lib.duplicates(EntityKind::Artist, None).unwrap();
        assert!(
            similar
                .iter()
                .any(|p| p.a.name.starts_with("Daft") && p.b.name.starts_with("Daft"))
        );
        assert!(
            !lib.duplicates(EntityKind::Track, None).unwrap().is_empty(),
            "the seeded second copies are suggested as duplicate tracks"
        );
        // a featured credit is an entry of its own, linked to the artist
        let featured: usize = lib
            .artists(&FilterSpec::default())
            .unwrap()
            .iter()
            .map(|a| a.featured)
            .sum();
        assert!(featured > 0);
        let latin = FilterSpec {
            tag_all: vec!["genre:latin".into()],
            ..Default::default()
        };
        assert!(
            lib.artists(&latin).unwrap().len() >= 3,
            "artists are reached through the tree"
        );
        let by_language = |t: &str| FilterSpec {
            tag_all: vec![t.into()],
            ..Default::default()
        };
        let total = |f: &FilterSpec| lib.search(f, Page::ALL).unwrap().total;
        let (romance, spanish) = (
            total(&by_language("lang:romance")),
            total(&by_language("lang:spanish")),
        );
        assert!(
            spanish > 0 && romance > spanish,
            "the language tree reaches French through Romance"
        );
        assert!(
            !lib.albums(
                &FilterSpec {
                    tag_all: vec!["live".into()],
                    ..Default::default()
                },
                None
            )
            .unwrap()
            .is_empty()
        );
        for p in lib.playlists().unwrap() {
            let (_, r) = lib.playlist_tracks(&p.name, Page::ALL).unwrap();
            assert!(r.total > 0, "playlist {} is empty", p.name);
        }
    }

    #[test]
    fn generated_slugs_are_unique() {
        let mut slugs: Vec<_> = tracks(MAX_COUNT).iter().map(slug_of).collect();
        slugs.sort();
        slugs.dedup();
        // the three second copies share a slug base with their originals
        assert_eq!(slugs.len(), MAX_COUNT - 3);
    }
}
