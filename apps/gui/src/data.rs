//! The library the views read: a snapshot of the engine's database (`Library::load`), with the
//! tracks addressed by their index in it. The slug (the engine's identity) is on each track, and
//! `backend` turns indexes back into slugs for the engine. The listening history is the engine's
//! too (`apply_history`); a track never played has 0 plays.

use iced::Color;
use iced_adaptive_design::primitives::tree::TreeNode;

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// The index in `Library::tracks`.
    pub id: usize,
    /// The engine's identity for the track.
    pub slug: String,
    pub year: Option<i32>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub tags: Vec<String>,
    pub bpm: f32,
    pub rating: u8,
    pub secs: u32,
    /// How rare it is among the library's tracks: 0 is the one added last (the engine's `Added`
    /// order, newest first).
    pub added: usize,
    /// Listening history, from the engine (`Library::apply_history`): times played, whole days
    /// since the last play, and when it was (unix seconds). 0 and 0 and 0 for a track never
    /// played.
    pub plays: u32,
    pub last_played_days: u32,
    pub played_at: i64,
}

impl Track {
    pub fn duration(&self) -> String {
        clock(self.secs as f32)
    }
}

pub fn clock(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The tags whose parent is `parent`, each with its children: the tag tree of the vocabulary.
fn tag_nodes(all: &[weave_player_lib::core::Tag], parent: Option<&str>) -> Vec<TreeNode> {
    all.iter()
        .filter(|t| t.parent.as_deref() == parent)
        .map(|t| {
            let kids = tag_nodes(all, Some(&t.name));
            let label = t.name.split_once(':').map_or(t.name.as_str(), |(_, v)| v);
            if kids.is_empty() {
                TreeNode::new(t.name.clone(), label)
            } else {
                TreeNode::branch(t.name.clone(), label, kids)
            }
        })
        .collect()
}

/// `tag` and everything under it in `tree`.
fn subtree<'a>(tree: &'a [TreeNode], tag: &str, out: &mut Vec<&'a str>, inside: bool) {
    for n in tree {
        let here = inside || n.value.eq_ignore_ascii_case(tag);
        if here {
            out.push(&n.value);
        }
        subtree(&n.children, tag, out, here);
    }
}

/// The filter page's state: tags (each also reaching its descendants) and a BPM range.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub tags: Vec<String>,
    pub bpm: (f32, f32),
}

pub const BPM: (f32, f32) = (60.0, 180.0);

impl Default for Filter {
    fn default() -> Self {
        Self {
            tags: Vec::new(),
            bpm: BPM,
        }
    }
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.bpm == BPM
    }

    /// The number of active conditions, for a badge.
    pub fn count(&self) -> usize {
        self.tags.len() + usize::from(self.bpm != BPM)
    }

    /// All the chosen tags must be on the track (or one of their descendants).
    pub fn matches(&self, t: &Track, tree: &[TreeNode]) -> bool {
        if self.bpm != BPM && (t.bpm < self.bpm.0 || t.bpm > self.bpm.1) {
            return false;
        }
        self.tags.iter().all(|want| {
            let mut covered = Vec::new();
            subtree(tree, want, &mut covered, false);
            t.tags
                .iter()
                .any(|have| covered.iter().any(|c| c.eq_ignore_ascii_case(have)))
        })
    }
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub name: String,
    /// An icon key (as for tags); a playlist without one is drawn as an avatar of its name.
    pub icon: Option<&'static str>,
    pub description: String,
    /// A dynamic playlist is a filter; a manual one an ordered list.
    pub filter: Option<Filter>,
    pub tracks: Vec<usize>,
}

/// How a tag is drawn: an icon key and a colour (the engine's `Tag::icon` and `Tag::color`).
#[derive(Debug, Clone, PartialEq)]
pub struct TagStyle {
    pub icon: String,
    pub color: Color,
}

/// `#rrggbb` as a colour.
pub fn parse_hex(hex: &str) -> Option<Color> {
    let h = hex.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6)?;
    Some(Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// The default look by tag key, for a tag that has none of its own: genre, mood, language, ...
pub fn default_style(tag: &str) -> TagStyle {
    let (icon, hex) = match tag
        .split_once(':')
        .map(|(k, _)| k.to_lowercase())
        .as_deref()
    {
        Some("genre") => ("note", "#8e5ad7"),
        Some("mood") => ("smile", "#e0709a"),
        Some("lang") | Some("language") => ("globe", "#2ec27e"),
        Some("energy") => ("bolt", "#f5a623"),
        Some("style") => ("star", "#c88a2e"),
        _ => ("tag", "#9a9996"),
    };
    TagStyle {
        icon: icon.into(),
        color: parse_hex(hex).unwrap_or(Color::from_rgb8(0x9a, 0x99, 0x96)),
    }
}

/// How "Recent tracks" is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecentOrder {
    /// What was played last (until something was played, what was added last).
    #[default]
    Played,
    /// What was added to the library last.
    Added,
}

/// Seconds since the Unix epoch.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub struct Library {
    pub tracks: Vec<Track>,
    pub styles: Vec<(String, TagStyle)>,
    pub tags: Vec<TreeNode>,
    pub playlists: Vec<Playlist>,
}

impl Library {
    /// Reads the whole library from the engine: tracks, the tag tree and the tags' looks, and the
    /// playlists.
    pub fn load(engine: &weave_player_lib::Library) -> weave_player_lib::core::Result<Self> {
        use weave_player_lib::core::{FilterSpec, Page, PlaylistKind, Sort, SortField};
        let found = engine.search(&FilterSpec::default(), Page::ALL)?;
        // The order tracks were added in, newest first, as a rank for each.
        let by_added = engine.search(
            &FilterSpec {
                sort: Some(Sort {
                    field: SortField::Added,
                    descending: true,
                }),
                ..FilterSpec::default()
            },
            Page::ALL,
        )?;
        let added: std::collections::HashMap<&str, usize> = by_added
            .tracks
            .iter()
            .enumerate()
            .map(|(rank, t)| (t.id.0.as_str(), rank))
            .collect();
        let tracks: Vec<Track> = found
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| Track {
                id: i,
                slug: t.id.0.clone(),
                year: t.metadata.year,
                title: t.metadata.title.clone(),
                artist: t
                    .metadata
                    .artist
                    .clone()
                    .unwrap_or_else(|| "Unknown artist".into()),
                album: t
                    .metadata
                    .album
                    .clone()
                    .unwrap_or_else(|| "Unknown album".into()),
                tags: t.tags.clone(),
                bpm: t.metadata.bpm.unwrap_or(0.0) as f32,
                rating: t.rating.unwrap_or(0),
                secs: (t.metadata.duration_ms.unwrap_or(0) / 1000) as u32,
                added: *added.get(t.id.0.as_str()).unwrap_or(&usize::MAX),
                plays: 0,
                last_played_days: 0,
                played_at: 0,
            })
            .collect();
        let index: std::collections::HashMap<&str, usize> =
            tracks.iter().map(|t| (t.slug.as_str(), t.id)).collect();

        let all = engine.list_tags()?;
        let styles = all
            .iter()
            .filter_map(|t| {
                let color = t.color.as_deref().and_then(parse_hex);
                match (t.icon.clone(), color) {
                    (None, None) => None,
                    (icon, color) => {
                        let d = default_style(&t.name);
                        Some((
                            t.name.clone(),
                            TagStyle {
                                icon: icon.unwrap_or(d.icon),
                                color: color.unwrap_or(d.color),
                            },
                        ))
                    }
                }
            })
            .collect();
        let tags = tag_nodes(&all, None);

        let mut playlists = Vec::new();
        for p in engine.playlists()? {
            let (_, result) = engine.playlist_tracks(&p.id, Page::ALL)?;
            let tracks = result
                .tracks
                .iter()
                .filter_map(|t| index.get(t.id.0.as_str()).copied())
                .collect();
            let filter = (p.kind == PlaylistKind::Dynamic).then(|| Filter {
                tags: p.filter.tag_all.clone(),
                bpm: (
                    p.filter.bpm_min.map_or(BPM.0, |v| v as f32),
                    p.filter.bpm_max.map_or(BPM.1, |v| v as f32),
                ),
            });
            playlists.push(Playlist {
                name: p.name.clone(),
                icon: None,
                description: p.description.clone().unwrap_or_default(),
                filter,
                tracks,
            });
        }
        let mut lib = Self {
            tracks,
            styles,
            tags,
            playlists,
        };
        lib.apply_history(&engine.play_history()?, unix_now());
        Ok(lib)
    }

    /// Takes the listening history from the engine: what was played, how often and when last.
    /// Tracks not in `stats` are marked never played.
    pub fn apply_history(&mut self, stats: &[weave_player_lib::core::PlayStat], now: i64) {
        let index: std::collections::HashMap<String, usize> =
            self.tracks.iter().map(|t| (t.slug.clone(), t.id)).collect();
        for t in &mut self.tracks {
            t.plays = 0;
            t.last_played_days = 0;
            t.played_at = 0;
        }
        for s in stats {
            if let Some(&i) = index.get(s.track.as_str()) {
                let t = &mut self.tracks[i];
                t.plays = s.plays;
                t.played_at = s.last_played_at;
                t.last_played_days = ((now - s.last_played_at).max(0) / 86_400) as u32;
            }
        }
    }

    /// The look of a tag: its own, or the default of its key.
    pub fn style_of(&self, tag: &str) -> TagStyle {
        self.styles
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(tag))
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| default_style(tag))
    }

    /// An album's year: the earliest among its tracks, 0 when none has one.
    pub fn album_year(&self, album: &str, artist: &str) -> i32 {
        self.tracks
            .iter()
            .filter(|t| t.album == album && t.artist == artist)
            .filter_map(|t| t.year)
            .min()
            .unwrap_or(0)
    }

    /// The year for display: `–` when no track of the album has one.
    pub fn year_label(&self, album: &str, artist: &str) -> String {
        match self.album_year(album, artist) {
            0 => "–".into(),
            y => y.to_string(),
        }
    }

    /// The average rating of an artist's tracks, 0.0 when none is rated.
    pub fn artist_rating(&self, artist: &str) -> f32 {
        let r: Vec<f32> = self
            .tracks
            .iter()
            .filter(|t| t.artist == artist && t.rating > 0)
            .map(|t| f32::from(t.rating))
            .collect();
        if r.is_empty() {
            0.0
        } else {
            r.iter().sum::<f32>() / r.len() as f32
        }
    }

    /// An artist's albums as indexes into `albums()`.
    pub fn albums_of(&self, artist: &str) -> Vec<usize> {
        self.albums()
            .iter()
            .enumerate()
            .filter(|(_, a)| a.1 == artist)
            .map(|(i, _)| i)
            .collect()
    }

    /// How many playlists hold at least one track of the album.
    pub fn playlists_with(&self, album: &str, artist: &str) -> usize {
        self.playlists
            .iter()
            .filter(|p| {
                self.playlist_tracks(p)
                    .iter()
                    .any(|t| t.album == album && t.artist == artist)
            })
            .count()
    }

    /// Popular albums of an artist: the best rated first, then the most in playlists.
    pub fn popular_albums(&self, artist: &str, n: usize) -> Vec<usize> {
        let albums = self.albums();
        let mut v: Vec<(usize, f32, usize)> = self
            .albums_of(artist)
            .into_iter()
            .map(|i| {
                let (name, _, _) = &albums[i];
                let ts: Vec<&Track> = self
                    .tracks
                    .iter()
                    .filter(|t| t.album == *name && t.artist == artist)
                    .collect();
                let avg =
                    ts.iter().map(|t| f32::from(t.rating)).sum::<f32>() / ts.len().max(1) as f32;
                (i, avg, self.playlists_with(name, artist))
            })
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
        v.into_iter().take(n).map(|x| x.0).collect()
    }

    /// An artist's newest albums first.
    pub fn latest_albums(&self, artist: &str, n: usize) -> Vec<usize> {
        let albums = self.albums();
        let mut v = self.albums_of(artist);
        v.sort_by_key(|i| std::cmp::Reverse(self.album_year(&albums[*i].0, &albums[*i].1)));
        v.truncate(n);
        v
    }

    /// Artists by how much they were played, most first.
    pub fn most_listened_artists(&self, n: usize) -> Vec<(usize, u32)> {
        let artists = self.artists();
        let mut v: Vec<(usize, u32)> = artists
            .iter()
            .enumerate()
            .map(|(i, (a, _))| {
                (
                    i,
                    self.tracks
                        .iter()
                        .filter(|t| t.artist == *a)
                        .map(|t| t.plays)
                        .sum(),
                )
            })
            .collect();
        v.retain(|(_, plays)| *plays > 0);
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }

    /// The tracks played most recently, newest first.
    pub fn recently_played(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().filter(|t| t.plays > 0).collect();
        v.sort_by(|a, b| {
            b.played_at
                .cmp(&a.played_at)
                .then(a.last_played_days.cmp(&b.last_played_days))
                .then(a.id.cmp(&b.id))
        });
        v.truncate(n);
        v
    }

    /// The tracks added to the library last, newest first.
    pub fn recently_added(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().collect();
        v.sort_by_key(|t| (t.added, t.id));
        v.truncate(n);
        v
    }

    /// Recent tracks: what was played last, or until anything was played what was added last.
    /// `order` chooses the view: played (with that fallback) or added.
    pub fn recent_tracks(&self, n: usize, order: RecentOrder) -> Vec<&Track> {
        match order {
            RecentOrder::Played => {
                let played = self.recently_played(n);
                if played.is_empty() {
                    self.recently_added(n)
                } else {
                    played
                }
            }
            RecentOrder::Added => self.recently_added(n),
        }
    }

    /// Artists by when one of their tracks was last played, most recent first (indexes into
    /// `artists()`).
    pub fn recent_artists(&self, n: usize) -> Vec<usize> {
        let artists = self.artists();
        let mut seen: Vec<usize> = Vec::new();
        for t in self.recently_played(self.tracks.len()) {
            if let Some(i) = artists.iter().position(|a| a.0 == t.artist)
                && !seen.contains(&i)
            {
                seen.push(i);
                if seen.len() == n {
                    break;
                }
            }
        }
        seen
    }

    pub fn most_listened_tracks(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().filter(|t| t.plays > 0).collect();
        v.sort_by(|a, b| b.plays.cmp(&a.plays).then(a.id.cmp(&b.id)));
        v.truncate(n);
        v
    }

    /// Tracks rated 4 or 5 that have not been played for more than `days` days.
    pub fn been_a_while(&self, days: u32, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self
            .tracks
            .iter()
            .filter(|t| t.plays > 0 && t.rating >= 4 && t.last_played_days > days)
            .collect();
        v.sort_by(|a, b| {
            b.last_played_days
                .cmp(&a.last_played_days)
                .then(a.id.cmp(&b.id))
        });
        v.truncate(n);
        v
    }

    /// Every tag of the vocabulary, a parent before its children.
    pub fn tag_names(&self) -> Vec<String> {
        fn walk(n: &[TreeNode], out: &mut Vec<String>) {
            for x in n {
                out.push(x.value.clone());
                walk(&x.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.tags, &mut out);
        out
    }

    /// How many tracks fall in each of `bins` even slices of `BPM`, among those that match the
    /// filter's tags (its own BPM range is ignored, so the whole distribution stays visible).
    pub fn bpm_histogram(&self, f: &Filter, bins: usize) -> Vec<usize> {
        let only_tags = Filter {
            tags: f.tags.clone(),
            bpm: BPM,
        };
        let width = (BPM.1 - BPM.0) / bins as f32;
        let mut out = vec![0; bins];
        for t in self.filtered(&only_tags) {
            let i = (((t.bpm - BPM.0) / width) as usize).min(bins - 1);
            out[i] += 1;
        }
        out
    }

    pub fn filtered(&self, f: &Filter) -> Vec<&Track> {
        self.tracks
            .iter()
            .filter(|t| f.matches(t, &self.tags))
            .collect()
    }

    pub fn playlist_tracks(&self, p: &Playlist) -> Vec<&Track> {
        match &p.filter {
            Some(f) => self.filtered(f),
            None => p
                .tracks
                .iter()
                .filter_map(|i| self.tracks.get(*i))
                .collect(),
        }
    }

    pub fn artists(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for t in &self.tracks {
            match out.iter_mut().find(|(a, _)| *a == t.artist) {
                Some((_, n)) => *n += 1,
                None => out.push((t.artist.clone(), 1)),
            }
        }
        out.sort();
        out
    }

    pub fn albums(&self) -> Vec<(String, String, usize)> {
        let mut out: Vec<(String, String, usize)> = Vec::new();
        for t in &self.tracks {
            match out
                .iter_mut()
                .find(|(a, b, _)| *a == t.album && *b == t.artist)
            {
                Some((_, _, n)) => *n += 1,
                None => out.push((t.album.clone(), t.artist.clone(), 1)),
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn been_a_while_is_highly_rated_and_not_played_for_the_given_days() {
        let lib = crate::backend::sample::backend().1;
        for days in [90, 180, 365] {
            let v = lib.been_a_while(days, 100);
            assert!(!v.is_empty() || days == 365);
            assert!(v.iter().all(|t| t.rating >= 4 && t.last_played_days > days));
        }
        // a longer wait never adds tracks
        assert!(lib.been_a_while(365, 100).len() <= lib.been_a_while(90, 100).len());
    }

    #[test]
    fn an_artist_has_an_aggregate_rating_and_albums_in_two_orders() {
        let lib = crate::backend::sample::backend().1;
        let (name, _) = lib.artists()[1].clone();
        let r = lib.artist_rating(&name);
        assert!((0.0..=5.0).contains(&r) && r > 0.0);
        let latest = lib.latest_albums(&name, 10);
        let years: Vec<i32> = latest
            .iter()
            .map(|i| {
                let a = &lib.albums()[*i];
                lib.album_year(&a.0, &a.1)
            })
            .collect();
        assert!(years.windows(2).all(|w| w[0] >= w[1]), "newest first");
        assert_eq!(lib.popular_albums(&name, 10).len(), latest.len());
    }

    #[test]
    fn recent_artists_are_distinct_and_the_latest_played_comes_first() {
        let lib = crate::backend::sample::backend().1;
        let r = lib.recent_artists(5);
        assert_eq!(r.len(), 5);
        let mut d = r.clone();
        d.sort();
        d.dedup();
        assert_eq!(d.len(), r.len());
        let first = &lib.artists()[r[0]].0;
        let newest = lib.recently_played(1)[0];
        assert_eq!(&newest.artist, first);
    }

    #[test]
    fn most_listened_is_ordered_by_plays() {
        let lib = crate::backend::sample::backend().1;
        let t = lib.most_listened_tracks(10);
        assert!(t.windows(2).all(|w| w[0].plays >= w[1].plays));
        let a = lib.most_listened_artists(10);
        assert!(a.windows(2).all(|w| w[0].1 >= w[1].1));
    }
}
