use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Case-insensitive (Unicode) tag key. Display spelling is stored separately.
pub fn normalize_tag(name: &str) -> String {
    name.trim().to_lowercase()
}

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

/// Structured, serializable filter shared by the CLI, GUI and saved playlists.
/// Groups combine with AND; empty/omitted groups do not restrict results.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterSpec {
    pub tag_all: Vec<String>,
    pub tag_any: Vec<String>,
    pub tag_none: Vec<String>,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
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
        for list in [&mut self.tag_all, &mut self.tag_any, &mut self.tag_none] {
            let mut seen = Vec::new();
            list.retain_mut(|t| {
                *t = t.trim().to_string();
                let key = normalize_tag(t);
                if key.is_empty() || seen.contains(&key) {
                    return false;
                }
                seen.push(key);
                true
            });
        }
        for field in [
            &mut self.title,
            &mut self.artist,
            &mut self.album,
            &mut self.genre,
        ] {
            if field.as_deref().is_some_and(|s| s.trim().is_empty()) {
                *field = None;
            }
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
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
        if let (Some(min), Some(max)) = (self.year_min, self.year_max)
            && min > max
        {
            return Err(Error::InvalidFilter(format!(
                "year-min ({min}) is greater than year-max ({max})"
            )));
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
        if !update.tag_none.is_empty() {
            self.tag_none = update.tag_none;
        }
        self.bpm_min = update.bpm_min.or(self.bpm_min);
        self.bpm_max = update.bpm_max.or(self.bpm_max);
        self.title = update.title.or(self.title);
        self.artist = update.artist.or(self.artist);
        self.album = update.album.or(self.album);
        self.genre = update.genre.or(self.genre);
        self.year_min = update.year_min.or(self.year_min);
        self.year_max = update.year_max.or(self.year_max);
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

    #[test]
    fn rejects_inverted_range() {
        let f = FilterSpec {
            bpm_min: Some(140.0),
            bpm_max: Some(90.0),
            ..Default::default()
        };
        assert!(f.validate().is_err());
    }

    #[test]
    fn accepts_equal_bounds() {
        let f = FilterSpec {
            bpm_min: Some(90.0),
            bpm_max: Some(90.0),
            ..Default::default()
        };
        assert!(f.validate().is_ok());
    }

    #[test]
    fn rejects_inverted_year_range() {
        let f = FilterSpec {
            year_min: Some(2000),
            year_max: Some(1990),
            ..Default::default()
        };
        assert!(f.validate().is_err());
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
    fn rejects_nan_and_negative() {
        let f = FilterSpec {
            bpm_min: Some(f64::NAN),
            ..Default::default()
        };
        assert!(f.validate().is_err());
        let f = FilterSpec {
            bpm_max: Some(-1.0),
            ..Default::default()
        };
        assert!(f.validate().is_err());
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
            bpm_max: Some(140.0),
            ..Default::default()
        };
        let upd = FilterSpec {
            bpm_min: Some(90.0),
            ..Default::default()
        };
        let m = base.merged(upd);
        assert_eq!(m.tag_all, vec!["a"]);
        assert_eq!(m.bpm_min, Some(90.0));
        assert_eq!(m.bpm_max, Some(140.0));
    }
}
