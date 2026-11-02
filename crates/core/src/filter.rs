use serde::{Deserialize, Serialize};

use crate::{Error, Result, TagName, is_numeric_key, normalize_tag, parse_tag_ref};

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

/// Inclusive numeric bounds on a keyed tag's value (`bpm:90`, `year:1999`, `tempo:120`).
/// Compared as numbers on an indexed column, never as text. Tracks without a numeric tag of
/// this key never match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagRange {
    pub key: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl TagRange {
    pub fn new(key: &str, min: Option<f64>, max: Option<f64>) -> Self {
        Self {
            key: key.into(),
            min,
            max,
        }
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
    /// Keyed tag conditions, ANDed with each other and with everything else.
    pub tag_clauses: Vec<TagClause>,
    /// Numeric ranges over keyed tag values; every range must match (AND).
    pub tag_range: Vec<TagRange>,
    /// Case-insensitive substrings of the track's identifying fields. Genre, year and BPM are
    /// tags: use `genre:bachata` or a `tag_range` over `bpm` / `year`.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// `Some(true)`: only favorites; `Some(false)`: only non-favorites.
    pub favorite: Option<bool>,
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
        for r in &mut self.tag_range {
            r.key = r.key.trim().to_lowercase();
        }
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
        for r in &self.tag_range {
            let named = TagName::parse_lossy(&format!("{}:0", r.key));
            if named.key.as_deref() != Some(r.key.as_str()) {
                return Err(Error::InvalidFilter(format!(
                    "\"{}\" is not a tag key",
                    r.key
                )));
            }
            if r.min.is_none() && r.max.is_none() {
                return Err(Error::InvalidFilter(format!(
                    "the {} range needs a minimum or a maximum",
                    r.key
                )));
            }
            for v in [r.min, r.max].into_iter().flatten() {
                if !v.is_finite() || (is_numeric_key(&r.key) && v < 0.0) {
                    return Err(Error::InvalidFilter(format!(
                        "the {} range has an invalid bound",
                        r.key
                    )));
                }
            }
            if let (Some(min), Some(max)) = (r.min, r.max)
                && min > max
            {
                return Err(Error::InvalidFilter(format!(
                    "the {} minimum ({min}) is greater than its maximum ({max})",
                    r.key
                )));
            }
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
        if !update.tag_clauses.is_empty() {
            self.tag_clauses = update.tag_clauses;
        }
        if !update.tag_range.is_empty() {
            self.tag_range = update.tag_range;
        }
        self.title = update.title.or(self.title);
        self.artist = update.artist.or(self.artist);
        self.album = update.album.or(self.album);
        self.favorite = update.favorite.or(self.favorite);
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

    fn ranged(key: &str, min: Option<f64>, max: Option<f64>) -> FilterSpec {
        FilterSpec {
            tag_range: vec![TagRange::new(key, min, max)],
            ..Default::default()
        }
    }

    #[test]
    fn rejects_inverted_range() {
        assert!(ranged("bpm", Some(140.0), Some(90.0)).validate().is_err());
        assert!(
            ranged("year", Some(2000.0), Some(1990.0))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn accepts_equal_bounds() {
        assert!(ranged("bpm", Some(90.0), Some(90.0)).validate().is_ok());
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
    fn rejects_nan_negative_and_empty_ranges() {
        assert!(ranged("bpm", Some(f64::NAN), None).validate().is_err());
        assert!(ranged("bpm", None, Some(-1.0)).validate().is_err());
        assert!(ranged("bpm", None, None).validate().is_err());
        assert!(ranged("bad key", Some(1.0), None).validate().is_err());
        assert!(ranged("tempo", Some(-5.0), None).validate().is_ok()); // free-form keys may be negative
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
    fn merge_retains_unspecified() {
        let base = FilterSpec {
            tag_all: vec!["a".into()],
            title: Some("x".into()),
            ..Default::default()
        };
        let m = base.merged(ranged("bpm", Some(90.0), Some(140.0)));
        assert_eq!(m.tag_all, vec!["a"]);
        assert_eq!(m.title.as_deref(), Some("x"));
        assert_eq!(
            m.tag_range,
            ranged("bpm", Some(90.0), Some(140.0)).tag_range
        );
    }
}
