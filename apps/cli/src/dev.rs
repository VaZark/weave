//! Debug-build-only helpers. Compiled out of release builds.

use weave::Library;
use weave::core::{
    FilterSpec, Metadata, Result, ScannedTrack, Sort, SortField, TagRange, track_slug,
};

pub const MAX_COUNT: usize = 100;

struct Artist {
    name: &'static str,
    genre: &'static str,
    bpm: (u32, u32),
    years: (i32, i32),
}

const ARTISTS: [Artist; 8] = [
    Artist {
        name: "Romeo Santos",
        genre: "Bachata",
        bpm: (95, 135),
        years: (2011, 2022),
    },
    Artist {
        name: "Aventura",
        genre: "Bachata",
        bpm: (90, 130),
        years: (1999, 2009),
    },
    Artist {
        name: "Kaysha",
        genre: "Zouk",
        bpm: (80, 110),
        years: (2006, 2014),
    },
    Artist {
        name: "Édith Piaf",
        genre: "Chanson",
        bpm: (70, 120),
        years: (1947, 1962),
    },
    Artist {
        name: "Daft Punk",
        genre: "Electronic",
        bpm: (110, 130),
        years: (1997, 2013),
    },
    Artist {
        name: "Nina Simone",
        genre: "Jazz",
        bpm: (60, 110),
        years: (1958, 1974),
    },
    Artist {
        name: "Buena Vista Social Club",
        genre: "Son",
        bpm: (85, 125),
        years: (1997, 2000),
    },
    Artist {
        name: "Rock & Roll Trio",
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

/// Deterministic fake tracks: unique per (artist, album, title), with a sprinkle
/// of missing fields so empty-value paths get exercised.
pub fn tracks(count: usize) -> Vec<ScannedTrack> {
    (0..count.min(MAX_COUNT))
        .map(|i| {
            let a = &ARTISTS[i % ARTISTS.len()];
            let span = |lo: u32, hi: u32| lo + (i as u32 * 37) % (hi - lo + 1);
            let title = format!("{} {}", ADJECTIVES[i % 10], NOUNS[i / 10 % 10]);
            ScannedTrack {
                path: format!("/weave-seed/{i:03}.mp3"), // no file is behind it
                metadata: Metadata {
                    title,
                    artist: (i % 13 != 12).then(|| a.name.to_string()),
                    album: (i % 5 != 4)
                        .then(|| ALBUMS[i / ARTISTS.len() % ALBUMS.len()].to_string()),
                    album_artist: (i % 5 != 4).then(|| a.name.to_string()),
                    genre: (i % 11 != 10).then(|| a.genre.to_string()),
                    composer: i.is_multiple_of(4).then(|| "A. Composer".to_string()),
                    year: Some(span(a.years.0 as u32, a.years.1 as u32) as i32),
                    track_number: Some((i / 24 + 1) as u32),
                    disc_number: Some(1),
                    duration_ms: Some(span(150, 330) as u64 * 1000),
                    bpm: (i % 7 != 6).then(|| span(a.bpm.0, a.bpm.1) as f64),
                },
            }
        })
        .collect()
}

fn tags_for(t: &ScannedTrack, i: usize) -> Vec<String> {
    let m = &t.metadata;
    let mut tags: Vec<String> = Vec::new();
    if let Some(g) = &m.genre {
        tags.push(g.to_lowercase());
    }
    if let Some(y) = m.year {
        tags.push(format!("{:02}s", y % 100 / 10 * 10));
    }
    tags.push(["romantic", "party", "melodic"][i % 3].into());
    if i.is_multiple_of(9) {
        tags.push("live".into());
    }
    if i.is_multiple_of(14) {
        tags.push("remix".into());
    }
    if i.is_multiple_of(6) {
        tags.push("Sensual".into());
    }
    tags
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

    let slug_of = |t: &ScannedTrack| {
        let m = &t.metadata;
        track_slug(m.artist.as_deref(), m.album.as_deref(), &m.title)
    };

    let mut tagged = 0;
    for (i, t) in fake.iter().enumerate() {
        let slug = slug_of(t);
        tagged += lib.add_tags(&slug, &tags_for(t, i))?;
        // Favorites and ratings are only seeded onto tracks that have neither yet, so
        // re-running never overwrites something changed by hand.
        let current = lib.track(&slug)?;
        if current.rating.is_none() && !current.favorite {
            if i.is_multiple_of(5) {
                lib.set_favorite(std::slice::from_ref(&slug), true)?;
            }
            if !i.is_multiple_of(3) {
                lib.set_rating(&slug, Some(1 + (i * 7 % 5) as u8))?;
            }
        }
    }

    let created = |r: weave::core::Result<weave::core::Playlist>| match r {
        Ok(_) => Ok(1),
        Err(weave::core::Error::AlreadyExists(_)) => Ok(0),
        Err(e) => Err(e),
    };
    let strs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let dynamic = [
        (
            "Bachata 90s",
            "Slow dance nights",
            FilterSpec {
                tag_all: strs(&["bachata", "90s"]),
                tag_exclude: strs(&["live", "remix"]),
                tag_range: vec![TagRange::new("bpm", Some(90.0), None)],
                ..Default::default()
            },
        ),
        (
            "Party starters",
            "Fast and fun",
            FilterSpec {
                tag_any: strs(&["party", "electronic"]),
                tag_range: vec![TagRange::new("bpm", Some(115.0), Some(140.0))],
                sort: Some(Sort {
                    field: SortField::Bpm,
                    descending: true,
                }),
                ..Default::default()
            },
        ),
        (
            "Old classics",
            "Before 1970",
            FilterSpec {
                tag_range: vec![TagRange::new("year", None, Some(1969.0))],
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
        (
            "Favorites",
            "Everything marked with a heart",
            FilterSpec {
                favorite: Some(true),
                sort: Some(Sort {
                    field: SortField::Title,
                    descending: false,
                }),
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
    if made == 1 {
        let picks: Vec<String> = fake.iter().take(8).rev().map(slug_of).collect();
        lib.playlist_add("road-trip", &picks)?;
    }
    playlists += made;

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
    use weave::core::Page;

    #[test]
    fn seed_is_idempotent_and_populates_everything() {
        let mut lib = Library::open_in_memory().unwrap();
        let first = seed(&mut lib, 60).unwrap();
        assert_eq!((first.added, first.skipped, first.playlists), (60, 0, 6));
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
        assert!(tracks.iter().any(|t| t.favorite) && tracks.iter().any(|t| t.rating.is_some()));
        for p in lib.playlists().unwrap() {
            let (_, r) = lib.playlist_tracks(&p.name, Page::ALL).unwrap();
            assert!(r.total > 0, "playlist {} is empty", p.name);
        }
    }

    #[test]
    fn generated_slugs_are_unique() {
        let mut slugs: Vec<_> = tracks(MAX_COUNT)
            .iter()
            .map(|t| {
                let m = &t.metadata;
                track_slug(m.artist.as_deref(), m.album.as_deref(), &m.title)
            })
            .collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), MAX_COUNT);
    }
}
