//! Printing for the in-depth views: one track, one artist, one album, and their lists.

use weave::core::{
    AlbumEntry, AlbumProfile, ArtistEntry, ArtistProfile, Count, SimilarArtists, Stats, Track,
    TrackDetail,
};

use crate::Failure;
use crate::output::{dash, duration_cell, out, print_columns};

/// `2026-11-02` from unix seconds (UTC).
fn date(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `1h12m`, `5m02s`, `45s`.
fn span(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s / 60 % 60) {
        (0, 0) => format!("{s}s"),
        (0, m) => format!("{m}m{:02}s", s % 60),
        (h, m) => format!("{h}h{m:02}m"),
    }
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        dash()
    } else {
        items.join(", ")
    }
}

fn counts(items: &[Count]) -> String {
    if items.is_empty() {
        dash()
    } else {
        items
            .iter()
            .map(|c| format!("{} ({})", c.name, c.count))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn range<T: std::fmt::Display + PartialEq>(lo: Option<T>, hi: Option<T>) -> String {
    match (lo, hi) {
        (Some(a), Some(b)) if a == b => a.to_string(),
        (Some(a), Some(b)) => format!("{a} to {b}"),
        _ => dash(),
    }
}

fn kv(rows: &[(&str, String)]) {
    for (k, v) in rows {
        outln!("  {k:<14}{v}");
    }
}

fn stats_rows(s: &Stats) -> Vec<(&'static str, String)> {
    let bpm = match (s.bpm_min, s.bpm_max, s.bpm_avg) {
        (Some(lo), Some(hi), Some(avg)) => format!("{lo} to {hi} (average {avg:.0})"),
        _ => dash(),
    };
    let rated = match s.rating_avg {
        Some(avg) => format!("{} of {} (average {avg:.1})", s.rated, s.tracks),
        None => format!("0 of {}", s.tracks),
    };
    vec![
        ("Tracks", format!("{}, {}", s.tracks, span(s.duration_ms))),
        ("Years", range(s.year_min, s.year_max)),
        ("BPM", bpm),
        ("Rated", rated),
        (
            "In library",
            range(s.first_added.map(date), s.last_added.map(date)),
        ),
    ]
}

pub fn print_track(json: bool, d: &TrackDetail) -> Result<(), Failure> {
    out(json, d, || {
        let t = &d.track;
        let m = &t.metadata;
        outln!("{}", m.title);
        let named = |r: &Option<weave::core::EntityRef>| {
            r.as_ref()
                .map_or_else(dash, |r| format!("{} ({})", r.name, r.id))
        };
        let track_no = match (m.disc_number, m.track_number) {
            (Some(d), Some(n)) => format!("track {n}, disc {d}"),
            (None, Some(n)) => format!("track {n}"),
            _ => dash(),
        };
        let playlists: Vec<String> = d
            .playlists
            .iter()
            .map(|p| format!("{} ({})", p.name, p.id))
            .collect();
        let queue: Vec<String> = d.queue_positions.iter().map(|p| p.to_string()).collect();
        kv(&[
            ("ID", t.id.0.clone()),
            ("Artist", named(&d.artist)),
            ("Album", named(&d.album)),
            ("Album artist", m.album_artist.clone().unwrap_or_else(dash)),
            ("Composer", m.composer.clone().unwrap_or_else(dash)),
            ("Position", track_no),
            ("Genre", m.genre.clone().unwrap_or_else(dash)),
            ("Year", m.year.map_or_else(dash, |y| y.to_string())),
            ("BPM", m.bpm.map_or_else(dash, |b| b.to_string())),
            ("Length", duration_cell(m.duration_ms)),
            (
                "Rating",
                t.rating.map_or_else(dash, |r| format!("{r} of 5")),
            ),
            ("Tags", list(&t.tags)),
            ("Playlists", list(&playlists)),
            (
                "In queue",
                if queue.is_empty() {
                    dash()
                } else {
                    format!("at {}", queue.join(", "))
                },
            ),
            ("File", d.path.clone().unwrap_or_else(dash)),
            ("Added", date(d.added_at)),
        ]);
    })
}

pub fn print_artist(json: bool, p: &ArtistProfile) -> Result<(), Failure> {
    out(json, p, || {
        outln!("{} ({})", p.name, p.id);
        let mut rows = vec![("Also known as", list(&p.aliases)), ("Tags", list(&p.tags))];
        rows.extend(stats_rows(&p.stats));
        rows.push(("Genres", counts(&p.genres)));
        rows.push(("Track tags", counts(&p.track_tags)));
        kv(&rows);
        outln!();
        if p.albums.is_empty() {
            outln!("No albums.");
        } else {
            print_album_table(&p.albums, true);
        }
        if p.loose_tracks > 0 {
            outln!("{} track(s) on no album.", p.loose_tracks);
        }
    })
}

pub fn print_album(json: bool, p: &AlbumProfile) -> Result<(), Failure> {
    out(json, p, || {
        outln!("{} ({})", p.title, p.id);
        let artist = p
            .artist
            .as_ref()
            .map_or_else(dash, |a| format!("{} ({})", a.name, a.id));
        let mut rows = vec![("Artist", artist), ("Tags", list(&p.tags))];
        rows.extend(stats_rows(&p.stats));
        rows.push(("Genres", counts(&p.genres)));
        rows.push(("Track tags", counts(&p.track_tags)));
        kv(&rows);
        outln!();
        let rows = p
            .tracks
            .iter()
            .map(|t: &Track| {
                vec![
                    t.metadata.disc_number.map_or_else(dash, |d| d.to_string()),
                    t.metadata.track_number.map_or_else(dash, |n| n.to_string()),
                    t.metadata.title.clone(),
                    duration_cell(t.metadata.duration_ms),
                    t.rating.map_or_else(dash, |r| r.to_string()),
                ]
            })
            .collect();
        print_columns(&["DISC", "#", "TITLE", "TIME", "RATING"], rows, &[], None);
    })
}

fn print_album_table(albums: &[AlbumEntry], hide_artist: bool) {
    let rows = albums
        .iter()
        .map(|a| {
            let mut row = vec![a.title.clone()];
            if !hide_artist {
                row.push(a.artist.as_ref().map_or_else(dash, |r| r.name.clone()));
            }
            row.push(a.year.map_or_else(dash, |y| y.to_string()));
            row.push(a.tracks.to_string());
            row.push(list(&a.tags));
            row
        })
        .collect();
    let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
    let headers: &[&str] = if hide_artist {
        &["ALBUM", "YEAR", "TRACKS", "TAGS"]
    } else {
        &["ALBUM", "ARTIST", "YEAR", "TRACKS", "TAGS"]
    };
    // The album ID is how you address it, so albums are always listed with it.
    print_columns(headers, rows, &[], Some(&ids));
}

pub fn print_artists(json: bool, show_ids: bool, artists: &[ArtistEntry]) -> Result<(), Failure> {
    out(json, &artists, || {
        let rows = artists
            .iter()
            .map(|a| {
                vec![
                    a.name.clone(),
                    a.albums.to_string(),
                    a.tracks.to_string(),
                    list(&a.tags),
                ]
            })
            .collect();
        let ids: Vec<String> = artists.iter().map(|a| a.id.clone()).collect();
        print_columns(
            &["ARTIST", "ALBUMS", "TRACKS", "TAGS"],
            rows,
            &[],
            show_ids.then_some(&ids[..]),
        );
    })
}

pub fn print_albums(json: bool, albums: &[AlbumEntry]) -> Result<(), Failure> {
    out(json, &albums, || print_album_table(albums, false))
}

pub fn print_similar(json: bool, pairs: &[SimilarArtists]) -> Result<(), Failure> {
    out(json, &pairs, || {
        if pairs.is_empty() {
            outln!("No artists with similar names.");
            return;
        }
        let rows = pairs
            .iter()
            .map(|p| {
                let side = |a: &ArtistEntry| format!("{} ({}, {} tracks)", a.name, a.id, a.tracks);
                vec![side(&p.a), side(&p.b)]
            })
            .collect();
        print_columns(&["ARTIST", "LOOKS LIKE"], rows, &[0, 1], None);
        outln!();
        outln!("Same person: weave-player artist merge <from> <into>");
        outln!("Different people: weave-player artist separate <a> <b>");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_spans() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_700_000_000), "2023-11-14");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(span(45_000), "45s");
        assert_eq!(span(302_000), "5m02s");
        assert_eq!(span(4_320_000), "1h12m");
        assert_eq!(range(Some(2000), Some(2000)), "2000");
        assert_eq!(range(Some(1999), Some(2009)), "1999 to 2009");
    }
}
