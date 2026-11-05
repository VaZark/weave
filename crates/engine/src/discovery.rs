//! Collection discovery shared by all frontends. Names use Unicode substring matching.
use crate::Library;
use weave_player_core::{
    AlbumEntry, ArtistEntry, FilterSpec, Page, Playlist, Result, SearchResult, Sort, SortField, Tag,
};

fn contains(value: &str, query: &str) -> bool {
    value.to_lowercase().contains(&query.trim().to_lowercase())
}

impl Library {
    pub fn search_artists(&self, query: &str, filter: &FilterSpec) -> Result<Vec<ArtistEntry>> {
        Ok(self
            .artists(filter)?
            .into_iter()
            .filter(|a| contains(&a.name, query))
            .collect())
    }
    pub fn search_albums(
        &self,
        query: &str,
        filter: &FilterSpec,
        artist: Option<&str>,
    ) -> Result<Vec<AlbumEntry>> {
        Ok(self
            .albums(filter, artist)?
            .into_iter()
            .filter(|a| {
                contains(&a.title, query)
                    || a.artist
                        .as_ref()
                        .is_some_and(|ar| contains(&ar.name, query))
            })
            .collect())
    }
    pub fn search_playlists(&self, query: &str) -> Result<Vec<Playlist>> {
        Ok(self
            .playlists()?
            .into_iter()
            .filter(|p| {
                contains(&p.name, query)
                    || p.description.as_ref().is_some_and(|s| contains(s, query))
            })
            .collect())
    }
    /// Named keys; unkeyed tags are selected with `category = Some("")` (Custom).
    pub fn tag_categories(&self) -> Result<Vec<Option<String>>> {
        let tags = self.list_tags()?;
        let mut keys: Vec<_> = tags.into_iter().map(|t| t.key).collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }
    /// Category filters the tag key, parent limits to direct children; neither changes selection.
    /// An empty parent selects roots. An omitted parent searches all levels.
    pub fn browse_tags(
        &self,
        query: &str,
        category: Option<&str>,
        parent: Option<&str>,
    ) -> Result<Vec<Tag>> {
        let root_only = parent == Some("");
        let parent = parent
            .filter(|p| !p.is_empty())
            .map(|p| self.tag(p))
            .transpose()?;
        Ok(self
            .list_tags()?
            .into_iter()
            .filter(|t| {
                let category_matches = category.is_none_or(|key| {
                    t.key.as_deref().unwrap_or("").to_lowercase() == key.trim().to_lowercase()
                });
                let parent_matches = if root_only {
                    t.parent.is_none()
                } else {
                    parent.as_ref().is_none_or(|p| {
                        t.parent.as_deref().is_some_and(|n| {
                            weave_player_core::normalize_tag(n)
                                == weave_player_core::normalize_tag(&p.name)
                        })
                    })
                };
                category_matches
                    && parent_matches
                    && (contains(&t.name, query) || contains(&t.value, query))
            })
            .collect())
    }
    /// Default Rediscover: tracks started before, inactive for 180 days, rating/completion/start ranking.
    /// Callers override the duration or sort using the same serializable filter as saved playlists.
    pub fn rediscover(&self, filter: &FilterSpec, page: Page) -> Result<SearchResult> {
        let defaults = FilterSpec {
            not_played_for_days: Some(180),
            has_played: Some(true),
            sort: Some(Sort {
                field: SortField::Preference,
                descending: true,
            }),
            ..Default::default()
        };
        self.search(&defaults.merged(filter.clone()), page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_player_core::{Metadata, ScannedTrack};
    fn library() -> Library {
        let mut lib = Library::open_in_memory().unwrap();
        lib.import_scanned(&[
            ScannedTrack {
                path: "/a".into(),
                metadata: Metadata {
                    title: "ÉtÉ".into(),
                    artist: Some("Alice".into()),
                    album: Some("Morning".into()),
                    ..Default::default()
                },
            },
            ScannedTrack {
                path: "/b".into(),
                metadata: Metadata {
                    title: "Winter".into(),
                    artist: Some("Bob".into()),
                    album: Some("Evening".into()),
                    ..Default::default()
                },
            },
        ])
        .unwrap();
        lib
    }
    #[test]
    fn all_collection_search_and_tag_browsing_are_shared() {
        let mut lib = library();
        lib.create_tag("mood:calm", None).unwrap();
        lib.create_tag("mood:sleepy", Some("mood:calm")).unwrap();
        lib.create_tag("custom note", None).unwrap();
        lib.create_tag("genre:ambient", None).unwrap();
        lib.add_tags("alice-morning-été", &["mood:calm".into()])
            .unwrap();
        let artist = lib.artists(&FilterSpec::default()).unwrap()[0].id.clone();
        lib.add_tags_to(
            weave_player_core::EntityKind::Artist,
            &artist,
            &["mood:calm".into()],
        )
        .unwrap();
        let tagged = FilterSpec {
            tag_all: vec!["mood:calm".into()],
            ..Default::default()
        };
        assert_eq!(lib.search_artists("AL", &tagged).unwrap().len(), 1);
        assert!(lib.search_artists("bob", &tagged).unwrap().is_empty());
        assert_eq!(
            lib.search_albums("ALICE", &FilterSpec::default(), None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            lib.search_albums("morning", &FilterSpec::default(), Some(&artist))
                .unwrap()
                .len(),
            1
        );
        lib.create_playlist("Sunrise", Some("Quiet mornings"), FilterSpec::default())
            .unwrap();
        assert_eq!(lib.search_playlists("quiet").unwrap().len(), 1);
        assert_eq!(lib.search_playlists("SUNRISE").unwrap().len(), 1);
        assert!(lib.search_playlists("absent").unwrap().is_empty());
        let query = FilterSpec {
            query: Some("été".into()),
            ..tagged
        };
        assert_eq!(lib.search(&query, Page::ALL).unwrap().total, 1);
        assert_eq!(
            lib.browse_tags("", Some("mood"), Some("mood:calm"))
                .unwrap()[0]
                .name,
            "mood:sleepy"
        );
        assert_eq!(lib.browse_tags("custom", Some(""), None).unwrap().len(), 1);
        assert_eq!(
            lib.browse_tags("", Some("mood"), Some("")).unwrap().len(),
            1
        );
        assert!(
            lib.browse_tags("not there", Some("mood"), None)
                .unwrap()
                .is_empty()
        );
        assert!(lib.tag_categories().unwrap().contains(&None));
        assert!(lib.tag_categories().unwrap().contains(&Some("mood".into())));
    }
    #[test]
    fn rediscover_defaults_and_custom_filters_stay_queries() {
        let mut lib = library();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        lib.storage
            .record_play("alice-morning-été", now - 200 * 86_400)
            .unwrap();
        lib.storage
            .record_play("bob-evening-winter", now - 10 * 86_400)
            .unwrap();
        lib.set_rating("alice-morning-été", Some(5)).unwrap();
        assert_eq!(
            lib.rediscover(&FilterSpec::default(), Page::ALL)
                .unwrap()
                .tracks[0]
                .id
                .0,
            "alice-morning-été"
        );
        let f = FilterSpec {
            not_played_for_days: Some(1),
            sort: Some(Sort {
                field: SortField::LastPlayed,
                descending: true,
            }),
            ..Default::default()
        };
        let found = lib.rediscover(&f, Page::ALL).unwrap();
        assert_eq!(found.total, 2);
        assert_eq!(found.tracks[0].id.0, "bob-evening-winter");
        lib.storage.record_play("alice-morning-été", now).unwrap();
        assert_eq!(
            lib.rediscover(&FilterSpec::default(), Page::ALL)
                .unwrap()
                .total,
            0
        );
        assert!(
            lib.rediscover(
                &FilterSpec {
                    query: Some("no match".into()),
                    ..f
                },
                Page::ALL
            )
            .unwrap()
            .tracks
            .is_empty()
        );
    }
    #[test]
    fn measured_listening_refreshes_rediscover_even_when_legacy_start_is_old() {
        let mut lib = library();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        lib.storage
            .record_play("alice-morning-été", now - 200 * 86_400)
            .unwrap();
        assert_eq!(
            lib.rediscover(&FilterSpec::default(), Page::ALL)
                .unwrap()
                .total,
            1
        );
        let event = lib
            .storage
            .begin_listening("alice-morning-été", 1, (now - 1) * 1000)
            .unwrap();
        lib.storage
            .report_listening(&event, now * 1000, 1000)
            .unwrap();
        assert_eq!(
            lib.rediscover(&FilterSpec::default(), Page::ALL)
                .unwrap()
                .total,
            0
        );
    }
}
