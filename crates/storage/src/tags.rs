//! Weave's binding of the `weaver` crate: which things carry tags, and their public handles.

use rusqlite::Transaction;
use weave_player_core::{ChildPolicy, EntityKind, Result, Tag, TagName};
use weaver::{Owner, Tags};

use crate::Storage;

pub(crate) const TRACK: Owner = Owner {
    name: "track",
    table: "track_tags",
    col: "track_id",
    owner_table: "tracks",
    owner_key: "id",
    key_type: "TEXT",
};
pub(crate) const ARTIST: Owner = Owner {
    name: "artist",
    table: "artist_tags",
    col: "artist_id",
    owner_table: "artists",
    owner_key: "id",
    key_type: "TEXT",
};
pub(crate) const ALBUM: Owner = Owner {
    name: "album",
    table: "album_tags",
    col: "album_id",
    owner_table: "albums",
    owner_key: "id",
    key_type: "TEXT",
};
pub(crate) const PLAYLIST: Owner = Owner {
    name: "playlist",
    table: "playlist_tags",
    col: "playlist_id",
    owner_table: "playlists",
    owner_key: "id",
    key_type: "TEXT",
};

static OWNERS: [Owner; 4] = [TRACK, ARTIST, ALBUM, PLAYLIST];
pub(crate) const TAGS: Tags = Tags::new(&OWNERS);

pub(crate) fn owner(kind: EntityKind) -> &'static Owner {
    match kind {
        EntityKind::Track => &OWNERS[0],
        EntityKind::Artist => &OWNERS[1],
        EntityKind::Album => &OWNERS[2],
        EntityKind::Playlist => &OWNERS[3],
    }
}

/// Assign a tag to something (by internal key). Returns whether the assignment is new.
pub(crate) fn assign(
    tx: &Transaction,
    kind: EntityKind,
    owner_key: &str,
    name: &TagName,
) -> Result<bool> {
    Ok(TAGS.assign(tx, owner(kind), owner_key, name)?)
}

impl Storage {
    /// Add tags to a track (deduplicated, case-insensitive). Returns how many were newly applied.
    pub fn add_tags(&mut self, track_id: &str, names: &[String]) -> Result<usize> {
        self.add_tags_to(EntityKind::Track, track_id, names)
    }

    /// Remove tag assignments from a track. Returns how many were removed.
    pub fn remove_tags(&mut self, track_id: &str, names: &[String]) -> Result<usize> {
        self.remove_tags_from(EntityKind::Track, track_id, names)
    }

    /// A track's tags, in display spelling.
    pub fn tags(&self, track_id: &str) -> Result<Vec<String>> {
        self.tags_of(EntityKind::Track, track_id)
    }

    /// The internal key of a track, artist, album or playlist given its public ID.
    pub(crate) fn resolve_entity(&self, kind: EntityKind, key: &str) -> Result<String> {
        match kind {
            EntityKind::Track => self.resolve_id(key),
            EntityKind::Artist => self.resolve_artist(key),
            EntityKind::Album => self.resolve_album(key),
            EntityKind::Playlist => self.playlist(key).map(|p| p.id),
        }
    }

    /// Add tags to a track, artist, album or playlist. Returns how many were newly applied.
    pub fn add_tags_to(&mut self, kind: EntityKind, id: &str, names: &[String]) -> Result<usize> {
        // Parse before resolving, so a bad tag name is reported first.
        for n in names {
            TagName::parse(n)?;
        }
        let key = self.resolve_entity(kind, id)?;
        Ok(TAGS.add(&mut self.conn, owner(kind), &key, names)?)
    }

    pub fn remove_tags_from(
        &mut self,
        kind: EntityKind,
        id: &str,
        names: &[String],
    ) -> Result<usize> {
        let key = self.resolve_entity(kind, id)?;
        Ok(TAGS.remove(&mut self.conn, owner(kind), &key, names)?)
    }

    /// The tags on a track, artist, album or playlist, in display spelling.
    pub fn tags_of(&self, kind: EntityKind, id: &str) -> Result<Vec<String>> {
        let key = self.resolve_entity(kind, id)?;
        Ok(TAGS.of(&self.conn, owner(kind), &key)?)
    }

    /// Create a tag, optionally under an existing parent.
    pub fn create_tag(&mut self, name: &str, parent: Option<&str>) -> Result<Tag> {
        Ok(TAGS.create(&mut self.conn, name, parent)?.into())
    }

    pub fn tag(&self, name: &str) -> Result<Tag> {
        Ok(TAGS.get(&self.conn, name)?.into())
    }

    /// All tags by name, with their parents and direct counts.
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        Ok(TAGS.list(&self.conn)?.into_iter().map(Tag::from).collect())
    }

    /// Set a tag's icon (a key or path) and colour (`#rrggbb`); `None` clears that part.
    pub fn set_tag_style(
        &mut self,
        name: &str,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<Tag> {
        Ok(TAGS.set_style(&mut self.conn, name, icon, color)?.into())
    }

    /// Set or clear (`None`) a tag's parent. Rejects self-parenting and any cycle; assignments
    /// are untouched, only what `has(parent)` reaches changes.
    pub fn set_tag_parent(&mut self, name: &str, parent: Option<&str>) -> Result<Tag> {
        Ok(TAGS.set_parent(&mut self.conn, name, parent)?.into())
    }

    /// Delete a tag and its assignments. `Detach` turns its children into roots; `Delete` also
    /// deletes every descendant. Returns how many tags were deleted.
    pub fn delete_tag(&mut self, name: &str, children: ChildPolicy) -> Result<usize> {
        Ok(TAGS.delete(&mut self.conn, name, children)?)
    }
}
