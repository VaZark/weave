//! The data the UI shows while the engine is not wired in yet: a deterministic fake library.
//! Everything the views read goes through these types, so swapping in `weave-player-lib` later
//! replaces this module and nothing else.

use iced::Color;
use iced_adaptive_design::primitives::tree::TreeNode;

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: usize,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub tags: Vec<String>,
    pub bpm: f32,
    pub rating: u8,
    pub secs: u32,
    /// Listening history (the engine will provide it: see docs/ui/home.md).
    pub plays: u32,
    pub last_played_days: u32,
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

/// The tag vocabulary as a tree: `genre:bachata` has two children. Values are the tags' names.
pub fn tag_tree() -> Vec<TreeNode> {
    let leaf = |v: &str| TreeNode::new(v, v.split_once(':').map_or(v, |(_, x)| x));
    let branch = |v: &str, kids: Vec<TreeNode>| {
        TreeNode::branch(v, v.split_once(':').map_or(v, |(_, x)| x), kids)
    };
    vec![
        branch(
            "genre:bachata",
            vec![
                leaf("genre:bachata-sensual"),
                leaf("genre:bachata-traditional"),
            ],
        ),
        leaf("genre:zouk"),
        leaf("genre:salsa"),
        branch("mood:calm", vec![leaf("mood:late-night")]),
        leaf("mood:romantic"),
        leaf("mood:energetic"),
        leaf("lang:spanish"),
        leaf("lang:french"),
        leaf("practice"),
    ]
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
        if t.bpm < self.bpm.0 || t.bpm > self.bpm.1 {
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

pub struct Library {
    pub tracks: Vec<Track>,
    pub styles: Vec<(String, TagStyle)>,
    pub tags: Vec<TreeNode>,
    pub playlists: Vec<Playlist>,
}

impl Library {
    pub fn fake() -> Self {
        const ARTISTS: [&str; 8] = [
            "Aventura",
            "Kaysha",
            "Romeo Santos",
            "Celia Cruz",
            "Dadju",
            "Prince Royce",
            "Grupo Extra",
            "Sael",
        ];
        const WORDS: [&str; 12] = [
            "Obsesión",
            "Noche",
            "Corazón",
            "Bailar",
            "Sombras",
            "Lluvia",
            "Fuego",
            "Luna",
            "Dime",
            "Volver",
            "Cielo",
            "Besos",
        ];
        const TAGS: [&[&str]; 8] = [
            &["genre:bachata-sensual", "mood:romantic", "lang:spanish"],
            &["genre:zouk", "mood:calm", "lang:french"],
            &["genre:bachata-traditional", "lang:spanish"],
            &["genre:salsa", "mood:energetic", "lang:spanish"],
            &["genre:zouk", "mood:late-night", "lang:french"],
            &["genre:bachata-sensual", "mood:calm"],
            &["genre:bachata", "practice"],
            &["genre:zouk", "mood:romantic"],
        ];
        let tracks: Vec<Track> = (0..60)
            .map(|i| {
                let a = i % ARTISTS.len();
                Track {
                    id: i,
                    title: format!(
                        "{} {}",
                        WORDS[(i * 5) % WORDS.len()],
                        WORDS[(i * 7 + 3) % WORDS.len()]
                    ),
                    artist: ARTISTS[a].into(),
                    album: format!("{} Vol. {}", WORDS[(a * 3) % WORDS.len()], i / 16 + 1),
                    tags: TAGS[(i * 3 + a) % TAGS.len()]
                        .iter()
                        .map(|t| t.to_string())
                        .collect(),
                    bpm: 78.0 + ((i * 37) % 90) as f32,
                    rating: ((i * 11) % 5) as u8 + 1,
                    secs: 170 + ((i * 53) % 130) as u32,
                    plays: ((i * 7919) % 41) as u32,
                    last_played_days: ((i * 53) % 400) as u32,
                }
            })
            .collect();
        let playlists = vec![
            Playlist {
                name: "Bachata 90s".into(),
                icon: Some("heart"),
                description: "Sensual and traditional bachata, slow to mid tempo.".into(),
                filter: Some(Filter {
                    tags: vec!["genre:bachata".into()],
                    bpm: (60.0, 120.0),
                }),
                tracks: Vec::new(),
            },
            Playlist {
                name: "Late night".into(),
                icon: Some("moon"),
                description: String::new(),
                filter: Some(Filter {
                    tags: vec!["mood:calm".into()],
                    bpm: BPM,
                }),
                tracks: Vec::new(),
            },
            Playlist {
                name: "Road trip".into(),
                icon: None,
                description: "For the long drive.".into(),
                filter: None,
                tracks: vec![3, 9, 14, 21, 22, 30, 41],
            },
        ];
        // A few tags with a look of their own; every other tag falls back to the default of its
        // key (`default_style`).
        let styles = [
            ("mood:romantic", "heart", "#ff6b9d"),
            ("mood:late-night", "moon", "#3b4a7a"),
            ("practice", "target", "#9a9996"),
        ]
        .into_iter()
        .filter_map(|(n, i, c)| {
            Some((
                n.to_string(),
                TagStyle {
                    icon: i.into(),
                    color: parse_hex(c)?,
                },
            ))
        })
        .collect();
        Self {
            tracks,
            styles,
            tags: tag_tree(),
            playlists,
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

    /// An album's year (fake: the engine will carry it).
    pub fn album_year(album: &str, artist: &str) -> i32 {
        let h = album
            .bytes()
            .chain(artist.bytes())
            .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        2000 + (h % 25) as i32
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
        v.sort_by_key(|i| std::cmp::Reverse(Self::album_year(&albums[*i].0, &albums[*i].1)));
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
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }

    /// The tracks played most recently, newest first.
    pub fn recently_played(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().collect();
        v.sort_by(|a, b| {
            a.last_played_days
                .cmp(&b.last_played_days)
                .then(a.id.cmp(&b.id))
        });
        v.truncate(n);
        v
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
        let mut v: Vec<&Track> = self.tracks.iter().collect();
        v.sort_by(|a, b| b.plays.cmp(&a.plays).then(a.id.cmp(&b.id)));
        v.truncate(n);
        v
    }

    /// Tracks rated 4 or 5 that have not been played for more than `days` days.
    pub fn been_a_while(&self, days: u32, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self
            .tracks
            .iter()
            .filter(|t| t.rating >= 4 && t.last_played_days > days)
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
    use super::*;

    #[test]
    fn been_a_while_is_highly_rated_and_not_played_for_the_given_days() {
        let lib = Library::fake();
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
        let lib = Library::fake();
        let (name, _) = lib.artists()[1].clone();
        let r = lib.artist_rating(&name);
        assert!((0.0..=5.0).contains(&r) && r > 0.0);
        let latest = lib.latest_albums(&name, 10);
        let years: Vec<i32> = latest
            .iter()
            .map(|i| {
                let a = &lib.albums()[*i];
                Library::album_year(&a.0, &a.1)
            })
            .collect();
        assert!(years.windows(2).all(|w| w[0] >= w[1]), "newest first");
        assert_eq!(lib.popular_albums(&name, 10).len(), latest.len());
    }

    #[test]
    fn recent_artists_are_distinct_and_the_latest_played_comes_first() {
        let lib = Library::fake();
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
        let lib = Library::fake();
        let t = lib.most_listened_tracks(10);
        assert!(t.windows(2).all(|w| w[0].plays >= w[1].plays));
        let a = lib.most_listened_artists(10);
        assert!(a.windows(2).all(|w| w[0].1 >= w[1].1));
    }
}
