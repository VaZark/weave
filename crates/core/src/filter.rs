use serde::{Deserialize, Serialize};

use crate::{Error, Result, TagName, normalize_tag, parse_tag_ref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Title,
    Artist,
    Album,
    Genre,
    Year,
    Bpm,
    Duration,
    Rating,
    /// When the track was first indexed.
    Added,
}
text_enum!(SortField {
    Title => "title",
    Artist => "artist",
    Album => "album",
    Genre => "genre",
    Year => "year",
    Bpm => "bpm",
    Duration => "duration",
    Rating => "rating",
    Added => "added",
});

/// Unknown values always sort last, in either direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub field: SortField,
    pub descending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagOp {
    /// Every value is present.
    All,
    /// At least one value is present.
    Any,
    /// No value is present.
    Exclude,
}

/// A keyed tag condition: the tags `key:value` for each key and value. A value counts as
/// present when any of the keys has it, so `genre,style` + `bachata` means `genre:bachata`
/// or `style:bachata`. Matches the tag and its descendants unless `exact`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagClause {
    pub op: TagOp,
    pub keys: Vec<String>,
    pub values: Vec<String>,
    #[serde(default)]
    pub exact: bool,
}

impl TagClause {
    /// Normalized tag names, one list of alternative keys per value.
    pub fn names(&self) -> Vec<Vec<String>> {
        self.values
            .iter()
            .map(|v| {
                self.keys
                    .iter()
                    .map(|k| normalize_tag(&format!("{k}:{v}")))
                    .collect()
            })
            .collect()
    }
}

/// Structured, serializable filter shared by the CLI, GUI and saved playlists.
/// Groups combine with AND; empty/omitted groups do not restrict results.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterSpec {
    /// Tag groups. `genre:bachata` matches the tag and its descendants (`has`);
    /// `=genre:bachata` matches explicit assignments of that tag only (`has_exact`).
    pub tag_all: Vec<String>,
    pub tag_any: Vec<String>,
    pub tag_exclude: Vec<String>,
    /// Playlists to draw from, by ID or name. Empty means the whole library. A track must be in at
    /// least one of them (OR); the rest of the filter then applies on top (AND). A manual playlist
    /// limits the candidates to its tracks, a dynamic one contributes what it matches now, with
    /// its own sources, sort and limit.
    pub sources: Vec<String>,
    /// Keyed tag conditions, ANDed with each other and with everything else.
    pub tag_clauses: Vec<TagClause>,
    /// Bounds on the track's own BPM and year (inclusive; an unknown value never matches).
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    /// Case-insensitive substrings of the track's identifying fields. The genre is a tag:
    /// use `genre:bachata`.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Inclusive, 1 to 5. Unrated tracks never match a rating bound.
    pub rating_min: Option<u8>,
    pub rating_max: Option<u8>,
    /// Result order; default is artist, album, title.
    pub sort: Option<Sort>,
    /// Cap on the number of matches (applied before paging).
    pub limit: Option<usize>,
}

impl FilterSpec {
    /// Trim, drop empties and duplicates (by normalized form, keeping first spelling).
    pub fn normalized(mut self) -> Self {
        for list in [&mut self.tag_all, &mut self.tag_any, &mut self.tag_exclude] {
            let mut seen: Vec<(bool, String)> = Vec::new();
            list.retain_mut(|t| {
                *t = t.trim().to_string();
                let (exact, norm) = parse_tag_ref(t);
                let key = (exact, norm);
                if key.1.is_empty() || seen.contains(&key) {
                    return false;
                }
                seen.push(key);
                true
            });
        }
        let mut seen = Vec::new();
        self.sources.retain_mut(|p| {
            *p = p.trim().to_string();
            let key = normalize_tag(p);
            if key.is_empty() || seen.contains(&key) {
                return false;
            }
            seen.push(key);
            true
        });
        for c in &mut self.tag_clauses {
            for list in [&mut c.keys, &mut c.values] {
                let mut seen = Vec::new();
                list.retain_mut(|t| {
                    *t = t.trim().to_string();
                    let norm = t.to_lowercase();
                    if norm.is_empty() || seen.contains(&norm) {
                        return false;
                    }
                    seen.push(norm);
                    true
                });
            }
            for k in &mut c.keys {
                *k = k.to_lowercase();
            }
        }
        self.tag_clauses
            .retain(|c| !c.keys.is_empty() && !c.values.is_empty());
        for field in [&mut self.title, &mut self.artist, &mut self.album] {
            if field.as_deref().is_some_and(|s| s.trim().is_empty()) {
                *field = None;
            }
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
        for c in &self.tag_clauses {
            for k in &c.keys {
                if TagName::parse_lossy(&format!("{k}:x")).key.as_deref() != Some(k.as_str()) {
                    return Err(Error::InvalidFilter(format!("\"{k}\" is not a tag key")));
                }
            }
        }
        for (name, v) in [("bpm-min", self.bpm_min), ("bpm-max", self.bpm_max)] {
            if let Some(v) = v
                && (!v.is_finite() || v < 0.0)
            {
                return Err(Error::InvalidFilter(format!(
                    "{name} must be a non-negative number"
                )));
            }
        }
        if let (Some(min), Some(max)) = (self.bpm_min, self.bpm_max)
            && min > max
        {
            return Err(Error::InvalidFilter(format!(
                "bpm-min ({min}) is greater than bpm-max ({max})"
            )));
        }
        if let (Some(min), Some(max)) = (self.year_min, self.year_max)
            && min > max
        {
            return Err(Error::InvalidFilter(format!(
                "year-min ({min}) is greater than year-max ({max})"
            )));
        }
        for (name, v) in [
            ("rating-min", self.rating_min),
            ("rating-max", self.rating_max),
        ] {
            if let Some(v) = v
                && !(1..=5).contains(&v)
            {
                return Err(Error::InvalidFilter(format!(
                    "{name} must be between 1 and 5"
                )));
            }
        }
        if let (Some(min), Some(max)) = (self.rating_min, self.rating_max)
            && min > max
        {
            return Err(Error::InvalidFilter(format!(
                "rating-min ({min}) is greater than rating-max ({max})"
            )));
        }
        if self.limit == Some(0) {
            return Err(Error::InvalidFilter("limit must be at least 1".into()));
        }
        Ok(())
    }

    /// Overlay `update` on `self`: groups/fields that `update` specifies replace
    /// the existing ones; everything else is retained.
    pub fn merged(mut self, update: FilterSpec) -> Self {
        let update = update.normalized();
        if !update.tag_all.is_empty() {
            self.tag_all = update.tag_all;
        }
        if !update.tag_any.is_empty() {
            self.tag_any = update.tag_any;
        }
        if !update.tag_exclude.is_empty() {
            self.tag_exclude = update.tag_exclude;
        }
        if !update.sources.is_empty() {
            self.sources = update.sources;
        }
        if !update.tag_clauses.is_empty() {
            self.tag_clauses = update.tag_clauses;
        }
        self.bpm_min = update.bpm_min.or(self.bpm_min);
        self.bpm_max = update.bpm_max.or(self.bpm_max);
        self.year_min = update.year_min.or(self.year_min);
        self.year_max = update.year_max.or(self.year_max);
        self.title = update.title.or(self.title);
        self.artist = update.artist.or(self.artist);
        self.album = update.album.or(self.album);
        self.rating_min = update.rating_min.or(self.rating_min);
        self.rating_max = update.rating_max.or(self.rating_max);
        self.sort = update.sort.or(self.sort);
        self.limit = update.limit.or(self.limit);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tracks_own_bpm_and_year_bounds_are_validated() {
        let bad = |f: FilterSpec| f.validate().is_err();
        assert!(bad(FilterSpec {
            bpm_min: Some(140.0),
            bpm_max: Some(90.0),
            ..Default::default()
        }));
        assert!(bad(FilterSpec {
            bpm_min: Some(f64::NAN),
            ..Default::default()
        }));
        assert!(bad(FilterSpec {
            bpm_max: Some(-1.0),
            ..Default::default()
        }));
        assert!(bad(FilterSpec {
            year_min: Some(2000),
            year_max: Some(1990),
            ..Default::default()
        }));
        let ok = FilterSpec {
            bpm_min: Some(90.0),
            bpm_max: Some(90.0),
            year_min: Some(1990),
            year_max: Some(1990),
            ..Default::default()
        };
        assert!(ok.validate().is_ok());
        let merged = FilterSpec {
            bpm_max: Some(140.0),
            ..Default::default()
        }
        .merged(FilterSpec {
            bpm_min: Some(90.0),
            ..Default::default()
        });
        assert_eq!((merged.bpm_min, merged.bpm_max), (Some(90.0), Some(140.0)));
    }

    #[test]
    fn validates_ratings_and_limit() {
        let bad = |f: FilterSpec| f.validate().is_err();
        assert!(bad(FilterSpec {
            rating_min: Some(0),
            ..Default::default()
        }));
        assert!(bad(FilterSpec {
            rating_max: Some(6),
            ..Default::default()
        }));
        assert!(bad(FilterSpec {
            rating_min: Some(4),
            rating_max: Some(2),
            ..Default::default()
        }));
        assert!(bad(FilterSpec {
            limit: Some(0),
            ..Default::default()
        }));
        let ok = FilterSpec {
            rating_min: Some(3),
            rating_max: Some(3),
            limit: Some(1),
            ..Default::default()
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn merge_overlays_sort_limit_and_ratings() {
        let sort = Sort {
            field: SortField::Rating,
            descending: true,
        };
        let base = FilterSpec {
            rating_min: Some(4),
            limit: Some(10),
            ..Default::default()
        };
        let m = base.merged(FilterSpec {
            sort: Some(sort),
            limit: Some(5),
            ..Default::default()
        });
        assert_eq!(
            (m.rating_min, m.limit, m.sort),
            (Some(4), Some(5), Some(sort))
        );
    }

    #[test]
    fn sort_fields_parse() {
        assert_eq!("BPM".parse::<SortField>().unwrap(), SortField::Bpm);
        assert!(
            "loudness"
                .parse::<SortField>()
                .unwrap_err()
                .contains("title, artist")
        );
    }

    #[test]
    fn normalized_dedupes_case_insensitively_keeping_first_spelling() {
        let f = FilterSpec {
            tag_all: vec![
                "Bachata".into(),
                " bachata ".into(),
                "".into(),
                "ÉTÉ".into(),
                "été".into(),
            ],
            ..Default::default()
        }
        .normalized();
        assert_eq!(f.tag_all, vec!["Bachata", "ÉTÉ"]);
    }

    #[test]
    fn sources_are_trimmed_deduplicated_and_replaced_by_a_merge() {
        let f = FilterSpec {
            sources: vec![
                " Road Trip ".into(),
                "road trip".into(),
                "".into(),
                "Mix".into(),
            ],
            ..Default::default()
        }
        .normalized();
        assert_eq!(f.sources, ["Road Trip", "Mix"]);
        let m = f.merged(FilterSpec {
            sources: vec!["Other".into()],
            ..Default::default()
        });
        assert_eq!(m.sources, ["Other"]);
    }

    #[test]
    fn merge_retains_unspecified() {
        let base = FilterSpec {
            tag_all: vec!["a".into()],
            title: Some("x".into()),
            ..Default::default()
        };
        let m = base.merged(FilterSpec {
            tag_any: vec!["b".into()],
            ..Default::default()
        });
        assert_eq!(
            (m.tag_all, m.tag_any),
            (vec!["a".to_string()], vec!["b".to_string()])
        );
        assert_eq!(m.title.as_deref(), Some("x"));
    }
}
