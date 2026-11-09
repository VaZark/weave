//! Tag conditions of a filter: flat lists and keyed clauses, independent of what is tagged.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, TagName, normalize_tag, parse_tag_ref};

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

/// All tag conditions of a filter, ANDed together. Empty groups restrict nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagFilter {
    /// Additional AND blocks, each containing OR alternatives. Leaves cannot contain blocks.
    pub blocks: Vec<Vec<TagFilter>>,
    /// `genre:bachata` matches the tag and its descendants; `=genre:bachata` matches explicit
    /// assignments of that tag only.
    pub all: Vec<String>,
    pub any: Vec<String>,
    pub exclude: Vec<String>,
    pub clauses: Vec<TagClause>,
}

impl TagFilter {
    /// Trim, drop empties and duplicates (by normalized form, keeping first spelling).
    pub fn normalized(mut self) -> Self {
        self.blocks = self
            .blocks
            .into_iter()
            .map(|b| {
                b.into_iter()
                    .map(Self::normalized)
                    .filter(|g| !g.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|b| !b.is_empty())
            .collect();
        for list in [&mut self.all, &mut self.any, &mut self.exclude] {
            let mut seen: Vec<(bool, String)> = Vec::new();
            list.retain_mut(|t| {
                *t = t.trim().to_string();
                let key = parse_tag_ref(t);
                if key.1.is_empty() || seen.contains(&key) {
                    return false;
                }
                seen.push(key);
                true
            });
        }
        for c in &mut self.clauses {
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
        self.clauses
            .retain(|c| !c.keys.is_empty() && !c.values.is_empty());
        self
    }

    pub fn validate(&self) -> Result<()> {
        for group in self.blocks.iter().flatten() {
            if !group.blocks.is_empty() {
                return Err(Error::InvalidFilter(
                    "Nested tag groups are not supported".into(),
                ));
            }
            group.validate()?;
        }
        for c in &self.clauses {
            for k in &c.keys {
                if TagName::parse_lossy(&format!("{k}:x")).key.as_deref() != Some(k.as_str()) {
                    return Err(Error::InvalidFilter(format!("\"{k}\" is not a tag key")));
                }
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.iter().flatten().all(Self::is_empty)
            && self.all.is_empty()
            && self.any.is_empty()
            && self.exclude.is_empty()
            && self.clauses.is_empty()
    }
}
