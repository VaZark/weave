//! App-specific: changing the tags of many things at once. Tags are compared without regard to
//! case, like the engine's tag identity. Three changes: **add** a tag to everything, and for a tag
//! that is already there, **delete** it or **replace** it with another (the match is on the
//! tag's name, so something without it is left alone).

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Keep,
    Delete,
    Replace(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub add: Vec<String>,
    /// For each existing tag, what to do where it is found.
    pub changes: Vec<(String, Change)>,
}

fn has(tags: &[String], name: &str) -> bool {
    tags.iter().any(|t| t.eq_ignore_ascii_case(name))
}

fn push_new(tags: &mut Vec<String>, name: &str) {
    if !has(tags, name) {
        tags.push(name.to_string());
    }
}

/// Applies the plan to one thing's tags: changes to existing tags first, then the additions.
pub fn apply(tags: &mut Vec<String>, plan: &Plan) {
    for (name, change) in &plan.changes {
        if !has(tags, name) {
            continue;
        }
        match change {
            Change::Keep => {}
            Change::Delete => tags.retain(|t| !t.eq_ignore_ascii_case(name)),
            Change::Replace(to) => {
                tags.retain(|t| !t.eq_ignore_ascii_case(name));
                push_new(tags, to);
            }
        }
    }
    for t in &plan.add {
        push_new(tags, t);
    }
}

/// The tags in use among `things`, with how many carry each, most common first.
pub fn census<'a>(things: impl Iterator<Item = &'a [String]>) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for tags in things {
        for t in tags {
            match out.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(t)) {
                Some((_, c)) => *c += 1,
                None => out.push((t.clone(), 1)),
            }
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_delete_and_replace_on_match() {
        let plan = Plan {
            add: v(&["practice", "MOOD:calm"]),
            changes: vec![
                ("genre:zouk".into(), Change::Delete),
                ("lang:french".into(), Change::Replace("lang:fr".into())),
                ("mood:late-night".into(), Change::Keep),
            ],
        };
        let mut a = v(&["genre:zouk", "lang:french", "mood:calm"]);
        apply(&mut a, &plan);
        assert_eq!(
            a,
            v(&["mood:calm", "lang:fr", "practice"]),
            "no duplicate of mood:calm"
        );

        // a track without the matched tags is only given the additions
        let mut b = v(&["genre:salsa"]);
        apply(&mut b, &plan);
        assert_eq!(b, v(&["genre:salsa", "practice", "MOOD:calm"]));
    }

    #[test]
    fn matching_ignores_case_and_replace_does_not_duplicate() {
        let plan = Plan {
            add: vec![],
            changes: vec![("Genre:Zouk".into(), Change::Replace("genre:kizomba".into()))],
        };
        let mut a = v(&["genre:zouk", "genre:kizomba"]);
        apply(&mut a, &plan);
        assert_eq!(a, v(&["genre:kizomba"]));
    }

    #[test]
    fn census_counts_tags_by_how_many_things_have_them() {
        let a = v(&["x", "Y"]);
        let b = v(&["y"]);
        let c = census([a.as_slice(), b.as_slice()].into_iter());
        assert_eq!(c, vec![("Y".into(), 2), ("x".into(), 1)]);
    }
}
