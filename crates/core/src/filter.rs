use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Case-insensitive (Unicode) tag key. Display spelling is stored separately.
pub fn normalize_tag(name: &str) -> String {
    name.trim().to_lowercase()
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
        for field in [&mut self.title, &mut self.artist, &mut self.album] {
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
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_inverted_range() {
        let f = FilterSpec { bpm_min: Some(140.0), bpm_max: Some(90.0), ..Default::default() };
        assert!(f.validate().is_err());
    }

    #[test]
    fn accepts_equal_bounds() {
        let f = FilterSpec { bpm_min: Some(90.0), bpm_max: Some(90.0), ..Default::default() };
        assert!(f.validate().is_ok());
    }

    #[test]
    fn rejects_nan_and_negative() {
        let f = FilterSpec { bpm_min: Some(f64::NAN), ..Default::default() };
        assert!(f.validate().is_err());
        let f = FilterSpec { bpm_max: Some(-1.0), ..Default::default() };
        assert!(f.validate().is_err());
    }

    #[test]
    fn normalized_dedupes_case_insensitively_keeping_first_spelling() {
        let f = FilterSpec {
            tag_all: vec!["Bachata".into(), " bachata ".into(), "".into(), "ÉTÉ".into(), "été".into()],
            ..Default::default()
        }
        .normalized();
        assert_eq!(f.tag_all, vec!["Bachata", "ÉTÉ"]);
    }

    #[test]
    fn merge_retains_unspecified() {
        let base = FilterSpec { tag_all: vec!["a".into()], bpm_max: Some(140.0), ..Default::default() };
        let upd = FilterSpec { bpm_min: Some(90.0), ..Default::default() };
        let m = base.merged(upd);
        assert_eq!(m.tag_all, vec!["a"]);
        assert_eq!(m.bpm_min, Some(90.0));
        assert_eq!(m.bpm_max, Some(140.0));
    }
}
