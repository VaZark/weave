//! The tag vocabulary: an optional key, a value, and the normalized identity derived from both.

use serde::{Deserialize, Serialize};

use crate::{Error, Metadata, Result};

/// Keys the importer and the query planner know about. Any other key is valid too.
pub const GENRE: &str = "genre";
pub const BPM: &str = "bpm";
pub const YEAR: &str = "year";

/// Blessed keys whose values must be numbers (and are range-queryable).
pub fn is_numeric_key(key: &str) -> bool {
    matches!(key, BPM | YEAR)
}

/// Prefix that makes a filter tag match explicit assignments only (`has_exact`).
pub const EXACT_PREFIX: char = '=';

/// A parsed tag name. The colon is display/query syntax only: `genre:bachata` is key `genre`,
/// value `bachata`, and means nothing hierarchical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagName {
    pub key: Option<String>,
    pub value: String,
}

fn valid_key(k: &str) -> bool {
    k.chars().any(char::is_alphabetic)
        && k.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

impl TagName {
    /// Split at the first colon when the text before it is a plain key; otherwise the whole
    /// text is an unkeyed value (so `Été` and `12:30 pm` stay valid). Lenient: no validation.
    pub fn parse_lossy(text: &str) -> Self {
        let text = text.trim();
        if let Some((k, v)) = text.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if valid_key(k) && !v.is_empty() {
                return Self {
                    key: Some(k.to_lowercase()),
                    value: v.to_string(),
                };
            }
        }
        Self {
            key: None,
            value: text.to_string(),
        }
    }

    /// Parse a tag to assign or create: non-empty, no leading `=`, numeric where the key says so.
    pub fn parse(text: &str) -> Result<Self> {
        let name = Self::parse_lossy(text);
        if name.value.is_empty() {
            return Err(Error::InvalidInput("tag names cannot be empty".into()));
        }
        if name.value.starts_with(EXACT_PREFIX) || name.display().starts_with(EXACT_PREFIX) {
            return Err(Error::InvalidInput(format!(
                "tag names cannot start with \"{EXACT_PREFIX}\" (it marks exact matches in filters)"
            )));
        }
        if let Some(k) = name.key.as_deref().filter(|k| is_numeric_key(k)) {
            match name.number() {
                Some(n) if n >= 0.0 => {}
                _ => {
                    return Err(Error::InvalidInput(format!(
                        "the value of a {k} tag must be a non-negative number, got \"{}\"",
                        name.value
                    )));
                }
            }
        }
        Ok(name)
    }

    /// Display spelling: lowercase key, value as written.
    pub fn display(&self) -> String {
        match &self.key {
            Some(k) => format!("{k}:{}", self.value),
            None => self.value.clone(),
        }
    }

    /// Case-insensitive (Unicode) lookup identity.
    pub fn normalized(&self) -> String {
        self.display().to_lowercase()
    }

    /// The value as a finite number, when it is one. Drives indexed range queries.
    pub fn number(&self) -> Option<f64> {
        self.value.parse::<f64>().ok().filter(|n| n.is_finite())
    }
}

/// Case-insensitive (Unicode) tag identity. Display spelling is stored separately.
pub fn normalize_tag(name: &str) -> String {
    TagName::parse_lossy(name).normalized()
}

/// A tag reference in a filter: `genre:bachata` (the tag and its descendants) or
/// `=genre:bachata` (explicit assignments of exactly that tag).
pub fn parse_tag_ref(text: &str) -> (bool, String) {
    let text = text.trim();
    match text.strip_prefix(EXACT_PREFIX) {
        Some(rest) => (true, normalize_tag(rest)),
        None => (false, normalize_tag(text)),
    }
}

/// A tag as stored: its name, parent and how many tracks are assigned it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Display name, e.g. `genre:Bachata`; also the tag's handle in the API.
    pub name: String,
    pub key: Option<String>,
    pub value: String,
    /// Display name of the parent tag; `None` for a root.
    pub parent: Option<String>,
    /// Tracks assigned this tag explicitly (descendants not counted).
    pub tracks: usize,
}

/// What a deleted tag's children become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildPolicy {
    /// Children become roots; nothing else is lost.
    Detach,
    /// Children and all their descendants are deleted too.
    Delete,
}

/// Tags that supported metadata fields map to: genre, year and bpm.
pub fn metadata_tags(m: &Metadata) -> Vec<TagName> {
    let mut out = Vec::new();
    let mut push = |key: &str, value: Option<String>| {
        if let Some(v) = value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            let name = TagName {
                key: Some(key.into()),
                value: v,
            };
            if TagName::parse(&name.display()).is_ok_and(|p| p == name) {
                out.push(name);
            }
        }
    };
    push(GENRE, m.genre.clone());
    push(YEAR, m.year.filter(|y| *y >= 0).map(|y| y.to_string()));
    push(
        BPM,
        m.bpm
            .filter(|b| b.is_finite() && *b >= 0.0)
            .map(|b| b.to_string()),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_and_unkeyed_names() {
        let k = TagName::parse("Genre: Bachata-Traditional").unwrap();
        assert_eq!(k.key.as_deref(), Some("genre"));
        assert_eq!(k.display(), "genre:Bachata-Traditional");
        assert_eq!(k.normalized(), "genre:bachata-traditional");
        let u = TagName::parse("Practice").unwrap();
        assert_eq!((u.key.clone(), u.normalized()), (None, "practice".into()));
        // not a key: stays whole
        assert_eq!(TagName::parse("12:30 pm").unwrap().key, None);
        assert_eq!(TagName::parse("a b:c").unwrap().key, None);
        // only the first colon splits
        assert_eq!(TagName::parse("artist:ac:dc").unwrap().value, "ac:dc");
    }

    #[test]
    fn validation() {
        assert!(TagName::parse("  ").is_err());
        assert!(TagName::parse("=x").is_err());
        assert!(TagName::parse("bpm:fast").is_err());
        assert!(TagName::parse("bpm:-3").is_err());
        assert!(TagName::parse("bpm:NaN").is_err());
        assert_eq!(TagName::parse("bpm:90.5").unwrap().number(), Some(90.5));
        assert!(TagName::parse("tempo:fast").is_ok()); // arbitrary keys stay free-form
    }

    #[test]
    fn refs_and_metadata() {
        assert_eq!(parse_tag_ref("=Genre:X"), (true, "genre:x".into()));
        assert_eq!(parse_tag_ref(" Genre:X "), (false, "genre:x".into()));
        let m = Metadata {
            title: "t".into(),
            genre: Some("Bachata".into()),
            year: Some(1999),
            bpm: Some(90.0),
            ..Default::default()
        };
        let names: Vec<_> = metadata_tags(&m).iter().map(TagName::display).collect();
        assert_eq!(names, ["genre:Bachata", "year:1999", "bpm:90"]);
    }
}
