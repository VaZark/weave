//! This instance's identity on the network and what it is allowed to do.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, slugify};

/// One running engine with its own library. Other instances (a phone, a second computer) find it
/// by `name` and, once trusted, control it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    /// Slug advertised to clients; the instance's public ID.
    pub name: String,
    /// May this instance play audio itself? Off makes it remote-only: it keeps a library and a
    /// queue and can be controlled, but never makes sound.
    pub playback: bool,
}

const MAX_NAME: usize = 48;

/// The advertised name for an instance nobody named: the machine's host name as a slug.
pub fn default_instance_name() -> String {
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| slugify(&h))
        .filter(|h| !h.is_empty());
    host.unwrap_or_else(|| "weave".into())
}

/// Normalize a chosen name to a slug; it must keep at least one letter or digit.
pub fn parse_instance_name(name: &str) -> Result<String> {
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(Error::InvalidInput(
            "an instance name needs at least one letter or digit".into(),
        ));
    }
    if slug.chars().count() > MAX_NAME {
        return Err(Error::InvalidInput(format!(
            "an instance name is at most {MAX_NAME} characters"
        )));
    }
    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_slugs() {
        assert_eq!(
            parse_instance_name(" Vaz's Desktop ").unwrap(),
            "vazs-desktop"
        );
        assert!(parse_instance_name("???").is_err());
        assert!(parse_instance_name(&"a".repeat(49)).is_err());
        assert!(!default_instance_name().is_empty());
    }
}
