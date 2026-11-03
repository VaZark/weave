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
    pub name: &'static str,
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
                }
            })
            .collect();
        let playlists = vec![
            Playlist {
                name: "Bachata 90s",
                icon: Some("heart"),
                description: "Sensual and traditional bachata, slow to mid tempo.".into(),
                filter: Some(Filter {
                    tags: vec!["genre:bachata".into()],
                    bpm: (60.0, 120.0),
                }),
                tracks: Vec::new(),
            },
            Playlist {
                name: "Late night",
                icon: Some("moon"),
                description: String::new(),
                filter: Some(Filter {
                    tags: vec!["mood:calm".into()],
                    bpm: BPM,
                }),
                tracks: Vec::new(),
            },
            Playlist {
                name: "Road trip",
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
