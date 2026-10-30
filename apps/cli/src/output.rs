//! Text and JSON rendering for the CLI.

use std::time::Duration;

use serde::Serialize;
use weave_core::{FilterSpec, PlayStatus, Playlist, PlaylistKind, SearchResult, Track};
use weave_engine::SessionView;

use crate::{Failure, PageArgs};

pub fn print_json<T: Serialize>(v: &T) -> Result<(), Failure> {
    let s = serde_json::to_string_pretty(v).map_err(|e| Failure::Other(e.to_string()))?;
    outln!("{s}");
    Ok(())
}

/// JSON when `--json`, otherwise the text rendering.
pub fn out<T: Serialize>(json: bool, v: &T, text: impl FnOnce()) -> Result<(), Failure> {
    if json {
        print_json(v)
    } else {
        text();
        Ok(())
    }
}

pub fn print_page(
    json: bool,
    show_ids: bool,
    args: PageArgs,
    r: &SearchResult,
) -> Result<(), Failure> {
    print_page_numbered(json, show_ids, args, r, false)
}

/// A playlist's tracks with their canonical play order (1-based, continuing across pages).
pub fn print_playlist_tracks(
    json: bool,
    show_ids: bool,
    args: PageArgs,
    r: &SearchResult,
) -> Result<(), Failure> {
    print_page_numbered(json, show_ids, args, r, true)
}

fn print_page_numbered(
    json: bool,
    show_ids: bool,
    args: PageArgs,
    r: &SearchResult,
    numbered: bool,
) -> Result<(), Failure> {
    let (page, per_page) = if args.all {
        (1, r.total.max(1))
    } else {
        (args.page as usize, args.per_page as usize)
    };
    let pages = r.total.div_ceil(per_page).max(1);
    let first = (page - 1) * per_page + 1;
    if json {
        let mut tracks = serde_json::json!(r.tracks);
        if numbered {
            for (i, t) in tracks.as_array_mut().into_iter().flatten().enumerate() {
                t["play_order"] = serde_json::json!(first + i);
            }
        }
        return print_json(&serde_json::json!({
            "total": r.total, "page": page, "pages": pages, "per_page": per_page, "tracks": tracks,
        }));
    }
    print_table(&r.tracks, show_ids, numbered.then_some(first));
    // Footer goes to stderr so stdout stays a clean table.
    if !args.all && pages > 1 {
        let hint = if page < pages {
            format!("; next: --page {}", page + 1)
        } else {
            String::new()
        };
        eprintln!("page {page} of {pages} ({} tracks){hint}", r.total);
    } else {
        eprintln!("{} track(s)", r.total);
    }
    Ok(())
}

fn dash() -> String {
    "-".to_string()
}

fn duration_cell(ms: Option<u64>) -> String {
    ms.map_or_else(dash, |ms| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60))
}

fn tags_cell(t: &Track) -> String {
    if t.tags.is_empty() {
        dash()
    } else {
        t.tags.join(",")
    }
}

fn favorite_cell(t: &Track) -> String {
    if t.favorite { "♥".into() } else { dash() }
}

fn rating_cell(t: &Track) -> String {
    t.rating.map_or_else(dash, |r| r.to_string())
}

fn print_table(tracks: &[Track], show_ids: bool, first_order: Option<usize>) {
    let mut rows: Vec<Vec<String>> = tracks
        .iter()
        .map(|t| {
            let m = &t.metadata;
            vec![
                m.title.clone(),
                m.artist.clone().unwrap_or_else(dash),
                m.album.clone().unwrap_or_else(dash),
                m.genre.clone().unwrap_or_else(dash),
                m.year.map_or_else(dash, |y| y.to_string()),
                m.bpm.map_or_else(dash, |b| format!("{b}")),
                duration_cell(m.duration_ms),
                rating_cell(t),
                favorite_cell(t),
                tags_cell(t),
            ]
        })
        .collect();
    let mut headers = vec![
        "TITLE", "ARTIST", "ALBUM", "GENRE", "YEAR", "BPM", "TIME", "RATING", "FAV", "TAGS",
    ];
    if let Some(first) = first_order {
        headers.insert(0, "#");
        for (i, row) in rows.iter_mut().enumerate() {
            row.insert(0, (first + i).to_string());
        }
    }
    let ids: Vec<String> = tracks.iter().map(|t| t.id.0.clone()).collect();
    print_columns(&headers, rows, &[], show_ids.then_some(&ids[..]));
}

/// Left-aligned table with a header row. Columns are capped at 32 characters
/// (ellipsized) except those listed in `uncapped`. `ids`, when given, becomes a
/// leading `ID` column that is never truncated, since it is meant to be copied.
pub fn print_columns(
    headers: &[&str],
    mut rows: Vec<Vec<String>>,
    uncapped: &[usize],
    ids: Option<&[String]>,
) {
    const MAX: usize = 32;
    let mut headers: Vec<&str> = headers.to_vec();
    let mut uncapped: Vec<usize> = uncapped.to_vec();
    if let Some(ids) = ids {
        headers.insert(0, "ID");
        for (row, id) in rows.iter_mut().zip(ids) {
            row.insert(0, id.clone());
        }
        uncapped = std::iter::once(0)
            .chain(uncapped.iter().map(|i| i + 1))
            .collect();
    }
    let widths: Vec<usize> = (0..headers.len())
        .map(|i| {
            let w = rows
                .iter()
                .map(|r| r[i].chars().count())
                .chain([headers[i].chars().count()])
                .max()
                .unwrap();
            if uncapped.contains(&i) { w } else { w.min(MAX) }
        })
        .collect();
    let line = |cells: Vec<&str>| {
        let parts: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| {
                let c = if c.chars().count() > *w {
                    c.chars().take(w - 1).chain(['…']).collect::<String>()
                } else {
                    c.to_string()
                };
                format!("{c:<w$}")
            })
            .collect();
        outln!("{}", parts.join("  ").trim_end());
    };
    line(headers.clone());
    for r in &rows {
        line(r.iter().map(String::as_str).collect());
    }
}

// --- playlists ---------------------------------------------------------------

pub fn playlist_json(p: &Playlist) -> serde_json::Value {
    serde_json::json!({
        "id": p.id, "name": p.name, "description": p.description,
        "type": p.kind.as_str(), "filter": p.filter,
    })
}

pub fn print_playlist(json: bool, show_ids: bool, p: &Playlist) -> Result<(), Failure> {
    out(json, &playlist_json(p), || {
        let id = if show_ids {
            format!("{}\t", p.id)
        } else {
            String::new()
        };
        outln!("{id}{}\t{}\t{}", p.name, p.kind, ruleset(p))
    })
}

pub fn print_playlists(json: bool, show_ids: bool, playlists: &[Playlist]) -> Result<(), Failure> {
    if json {
        return print_json(&playlists.iter().map(playlist_json).collect::<Vec<_>>());
    }
    let rows = playlists
        .iter()
        .map(|p| {
            vec![
                p.name.clone(),
                p.description.clone().unwrap_or_else(dash),
                p.kind.to_string(),
                ruleset(p),
            ]
        })
        .collect();
    let ids: Vec<String> = playlists.iter().map(|p| p.id.clone()).collect();
    print_columns(
        &["NAME", "DESCRIPTION", "TYPE", "RULESET"],
        rows,
        &[3],
        show_ids.then_some(&ids[..]),
    );
    Ok(())
}

fn ruleset(p: &Playlist) -> String {
    match p.kind {
        PlaylistKind::Dynamic => describe(&p.filter),
        PlaylistKind::Manual => dash(),
    }
}

/// A filter as the CLI flags that would create it.
pub fn describe(f: &FilterSpec) -> String {
    let mut parts = Vec::new();
    for (k, v) in [
        ("tag-all", &f.tag_all),
        ("tag-any", &f.tag_any),
        ("tag-none", &f.tag_none),
    ] {
        if !v.is_empty() {
            parts.push(format!("--{k} {}", v.join(",")));
        }
    }
    let numbers = [
        ("bpm-min", f.bpm_min.map(|v| v.to_string())),
        ("bpm-max", f.bpm_max.map(|v| v.to_string())),
        ("year-min", f.year_min.map(|v| v.to_string())),
        ("year-max", f.year_max.map(|v| v.to_string())),
        ("rating-min", f.rating_min.map(|v| v.to_string())),
        ("rating-max", f.rating_max.map(|v| v.to_string())),
    ];
    for (k, v) in numbers {
        if let Some(v) = v {
            parts.push(format!("--{k} {v}"));
        }
    }
    for (k, v) in [
        ("title", &f.title),
        ("artist", &f.artist),
        ("album", &f.album),
        ("genre", &f.genre),
    ] {
        if let Some(v) = v {
            parts.push(format!("--{k} {v:?}"));
        }
    }
    match f.favorite {
        Some(true) => parts.push("--favorite".into()),
        Some(false) => parts.push("--not-favorite".into()),
        None => {}
    }
    if let Some(s) = f.sort {
        let desc = if s.descending { " --desc" } else { "" };
        parts.push(format!("--sort {}{desc}", s.field));
    }
    if let Some(n) = f.limit {
        parts.push(format!("--limit {n}"));
    }
    if parts.is_empty() {
        "(all tracks)".into()
    } else {
        parts.join(" ")
    }
}

// --- playback session ----------------------------------------------------------

fn human(d: Duration) -> String {
    let s = d.as_secs();
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, _) => format!("{h}h {m:02}m"),
    }
}

fn sleep_label(v: &SessionView) -> String {
    match (v.state.sleep, v.sleep_remaining, v.state.stop_after_track) {
        (Some(t), Some(left), _) => {
            let tail = if t.stop_after {
                ", then stops after the track"
            } else {
                ""
            };
            format!("{} left{tail}", human(left))
        }
        (_, _, true) => "stops after this track".into(),
        _ => dash(),
    }
}

fn clock(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

fn time_label(v: &SessionView) -> String {
    let Some(t) = v.current() else { return dash() };
    let pos = clock(v.position.as_millis() as u64);
    match t.metadata.duration_ms {
        Some(len) => format!("{pos} / {}", clock(len)),
        None => pos,
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn track_label(t: &Track) -> String {
    match &t.metadata.artist {
        Some(a) => format!("{} ({a})", t.metadata.title),
        None => t.metadata.title.clone(),
    }
}

fn session_json(v: &SessionView, with_tracks: bool) -> serde_json::Value {
    let mut j = serde_json::json!({
        "status": v.state.status,
        "position": v.state.current.map(|c| c + 1),
        "length": v.tracks.len(),
        "current": v.current(),
        "shuffle": v.state.shuffle,
        "repeat": v.state.repeat,
        "end_of_queue": v.state.end_of_queue,
        "position_ms": v.position.as_millis() as u64,
        "volume": v.state.volume,
        "sleep_remaining_secs": v.sleep_remaining.map(|d| d.as_secs()),
        "sleep_stop_after": v.state.sleep.map(|t| t.stop_after),
        "stop_after_track": v.state.stop_after_track,
    });
    if with_tracks {
        j["tracks"] = serde_json::json!(v.tracks);
    }
    j
}

/// The "now playing" summary, shared by `now` and every command that changes the session.
pub fn print_session(
    json: bool,
    show_ids: bool,
    host_running: Option<bool>,
    v: &SessionView,
) -> Result<(), Failure> {
    let mut j = session_json(v, false);
    if let Some(running) = host_running {
        j["host_running"] = serde_json::json!(running);
    }
    out(json, &j, || {
        let current = v.current();
        let mut rows = vec![
            ("Status", v.state.status.to_string()),
            ("Track", current.map_or_else(dash, track_label)),
        ];
        if show_ids {
            rows.push(("ID", current.map_or_else(dash, |t| t.id.0.clone())));
        }
        rows.extend([
            (
                "Position",
                v.state
                    .current
                    .map_or_else(dash, |c| format!("{} of {}", c + 1, v.tracks.len())),
            ),
            ("Shuffle", on_off(v.state.shuffle).into()),
            ("Repeat", v.state.repeat.to_string()),
            ("End of queue", v.state.end_of_queue.to_string()),
            ("Time", time_label(v)),
            ("Volume", format!("{}%", (v.state.volume * 100.0).round())),
            ("Sleep timer", sleep_label(v)),
        ]);
        if let Some(running) = host_running {
            rows.push((
                "Host",
                if running { "running" } else { "not running" }.into(),
            ));
        }
        for (k, val) in rows {
            outln!("{k:<13} {val}");
        }
    })
}

pub fn print_queue(json: bool, show_ids: bool, v: &SessionView) -> Result<(), Failure> {
    if json {
        return print_json(&session_json(v, true));
    }
    let rows = v
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let marker = if v.state.current == Some(i) {
                match v.state.status {
                    PlayStatus::Playing => "▶",
                    PlayStatus::Paused => "⏸",
                    PlayStatus::Stopped => "■",
                }
            } else {
                " "
            };
            let m = &t.metadata;
            vec![
                format!("{marker} {}", i + 1),
                m.title.clone(),
                m.artist.clone().unwrap_or_else(dash),
                m.album.clone().unwrap_or_else(dash),
                m.bpm.map_or_else(dash, |b| format!("{b}")),
                duration_cell(m.duration_ms),
                tags_cell(t),
            ]
        })
        .collect();
    let ids: Vec<String> = v.tracks.iter().map(|t| t.id.0.clone()).collect();
    print_columns(
        &["#", "TITLE", "ARTIST", "ALBUM", "BPM", "TIME", "TAGS"],
        rows,
        &[],
        show_ids.then_some(&ids[..]),
    );
    eprintln!(
        "{} track(s) · shuffle {} · repeat {} · end of queue: {}",
        v.tracks.len(),
        on_off(v.state.shuffle),
        v.state.repeat,
        v.state.end_of_queue
    );
    Ok(())
}
