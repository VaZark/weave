//! Shared ordering for collection browsing; no frontend computes aggregate rankings.
use crate::Library;
use std::collections::HashMap;
use weave_player_core::{
    AlbumEntry, ArtistEntry, CollectionOrder, FilterSpec, Page, Playlist, Result, Track,
};

#[derive(Default)]
struct Rank {
    played: Option<i64>,
    added: Option<i64>,
    release: Option<i32>,
    rating: Option<f64>,
}
impl Rank {
    fn of(tracks: &[Track], history: &HashMap<String, i64>) -> Self {
        let rated: Vec<_> = tracks.iter().filter_map(|t| t.rating).collect();
        Self {
            played: tracks
                .iter()
                .filter_map(|t| history.get(&t.id.0).copied())
                .max(),
            added: tracks.iter().map(|t| t.added_at).filter(|v| *v > 0).max(),
            release: tracks.iter().filter_map(|t| t.metadata.year).max(),
            rating: (!rated.is_empty())
                .then(|| rated.iter().map(|v| f64::from(*v)).sum::<f64>() / rated.len() as f64),
        }
    }
    fn cmp(&self, other: &Self, order: CollectionOrder, descending: bool) -> std::cmp::Ordering {
        fn known<T: PartialOrd>(
            a: Option<T>,
            b: Option<T>,
            descending: bool,
        ) -> std::cmp::Ordering {
            use std::cmp::Ordering;
            match (a, b) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => if descending {
                    b.partial_cmp(&a)
                } else {
                    a.partial_cmp(&b)
                }
                .unwrap_or(Ordering::Equal),
            }
        }
        match order {
            CollectionOrder::Recent => known(self.played, other.played, descending).then(known(
                self.added,
                other.added,
                descending,
            )),
            CollectionOrder::ReleaseDate => known(self.release, other.release, descending),
            CollectionOrder::AddedDate => known(self.added, other.added, descending),
            CollectionOrder::Rating => known(self.rating, other.rating, descending),
        }
    }
}
impl Library {
    fn order_collection<T>(
        &self,
        items: Vec<T>,
        order: CollectionOrder,
        descending: bool,
        tracks: impl Fn(&T) -> Result<Vec<Track>>,
    ) -> Result<Vec<T>> {
        let history = if order == CollectionOrder::Recent {
            self.play_history()?
                .into_iter()
                .filter(|h| h.plays > 0)
                .map(|h| (h.track, h.last_played_at))
                .collect()
        } else {
            HashMap::new()
        };
        let mut ranked = items
            .into_iter()
            .map(|item| {
                let rank = Rank::of(&tracks(&item)?, &history);
                Ok((item, rank))
            })
            .collect::<Result<Vec<_>>>()?;
        // Stable ties retain the engine's normal name/ID order; missing values stay last.
        ranked.sort_by(|a, b| a.1.cmp(&b.1, order, descending));
        Ok(ranked.into_iter().map(|(item, _)| item).collect())
    }
    /// Rank an already scoped track set without changing its membership.
    pub fn order_tracks(&self, tracks: Vec<Track>, order: CollectionOrder) -> Result<Vec<Track>> {
        self.order_tracks_direction(tracks, order, true)
    }
    pub fn order_tracks_direction(
        &self,
        tracks: Vec<Track>,
        order: CollectionOrder,
        descending: bool,
    ) -> Result<Vec<Track>> {
        self.order_collection(tracks, order, descending, |t| Ok(vec![t.clone()]))
    }
    pub fn ordered_tracks(&self, query: &str, order: CollectionOrder) -> Result<Vec<Track>> {
        self.ordered_tracks_direction(query, order, true)
    }
    pub fn ordered_tracks_direction(
        &self,
        query: &str,
        order: CollectionOrder,
        descending: bool,
    ) -> Result<Vec<Track>> {
        let tracks = self
            .search(
                &FilterSpec {
                    query: Some(query.into()),
                    ..Default::default()
                },
                Page::ALL,
            )?
            .tracks;
        self.order_tracks_direction(tracks, order, descending)
    }
    pub fn ordered_artists(&self, query: &str, order: CollectionOrder) -> Result<Vec<ArtistEntry>> {
        self.ordered_artists_direction(query, order, true)
    }
    pub fn ordered_artists_direction(
        &self,
        query: &str,
        order: CollectionOrder,
        descending: bool,
    ) -> Result<Vec<ArtistEntry>> {
        self.order_collection(
            self.search_artists(query, &Default::default())?,
            order,
            descending,
            |artist| self.storage.artist_tracks(&artist.id),
        )
    }
    pub fn ordered_albums(&self, query: &str, order: CollectionOrder) -> Result<Vec<AlbumEntry>> {
        self.ordered_albums_direction(query, order, true)
    }
    pub fn ordered_albums_direction(
        &self,
        query: &str,
        order: CollectionOrder,
        descending: bool,
    ) -> Result<Vec<AlbumEntry>> {
        self.order_collection(
            self.search_albums(query, &Default::default(), None)?,
            order,
            descending,
            |album| Ok(self.album(&album.id)?.tracks),
        )
    }
    pub fn ordered_playlists(&self, query: &str, order: CollectionOrder) -> Result<Vec<Playlist>> {
        self.ordered_playlists_direction(query, order, true)
    }
    pub fn ordered_playlists_direction(
        &self,
        query: &str,
        order: CollectionOrder,
        descending: bool,
    ) -> Result<Vec<Playlist>> {
        self.order_collection(
            self.search_playlists(query)?,
            order,
            descending,
            |playlist| Ok(self.playlist_tracks(&playlist.id, Page::ALL)?.1.tracks),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_player_core::{Metadata, ScannedTrack};
    #[test]
    fn collection_orders_share_real_history_and_metadata_with_unknowns_last() {
        let mut lib = Library::open_in_memory().unwrap();
        for (name, year) in [("A", Some(2020)), ("B", Some(2025)), ("C", None)] {
            lib.import_scanned(&[ScannedTrack {
                path: format!("/{name}"),
                metadata: Metadata {
                    title: name.into(),
                    artist: Some(name.into()),
                    album: Some(name.into()),
                    year,
                    ..Default::default()
                },
            }])
            .unwrap();
        }
        let mut tracks = lib
            .ordered_tracks("", CollectionOrder::ReleaseDate)
            .unwrap();
        assert_eq!(
            tracks
                .iter()
                .map(|t| t.metadata.title.as_str())
                .collect::<Vec<_>>(),
            ["B", "A", "C"]
        );
        assert_eq!(
            lib.ordered_tracks_direction("", CollectionOrder::ReleaseDate, false)
                .unwrap()
                .iter()
                .map(|t| t.metadata.title.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
        let a = tracks.remove(1).id.0;
        let b = tracks.remove(0).id.0;
        lib.set_rating(&a, Some(5)).unwrap();
        lib.set_rating(&b, Some(3)).unwrap();
        lib.storage.record_play(&a, 100).unwrap();
        lib.storage.record_play(&b, 200).unwrap();
        let names = |order| {
            lib.ordered_artists("", order)
                .unwrap()
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(CollectionOrder::Recent), ["B", "A", "C"]);
        assert_eq!(names(CollectionOrder::Rating), ["A", "B", "C"]);
        assert_eq!(
            lib.ordered_artists_direction("", CollectionOrder::Rating, false)
                .unwrap()
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            ["B", "A", "C"]
        );

        assert_eq!(
            lib.ordered_albums("", CollectionOrder::ReleaseDate)
                .unwrap()[0]
                .title,
            "B"
        );
        assert_eq!(
            lib.ordered_albums("A", CollectionOrder::Rating)
                .unwrap()
                .len(),
            1
        );
        lib.create_manual_playlist("Five stars", None).unwrap();
        lib.playlist_add("Five stars", std::slice::from_ref(&a))
            .unwrap();
        lib.create_manual_playlist("Three stars", None).unwrap();
        lib.playlist_add("Three stars", std::slice::from_ref(&b))
            .unwrap();
        lib.create_manual_playlist("Empty", None).unwrap();
        assert_eq!(
            lib.ordered_playlists("", CollectionOrder::Rating)
                .unwrap()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["Five stars", "Three stars", "Empty"]
        );
        assert_eq!(
            lib.ordered_playlists_direction("", CollectionOrder::Rating, false)
                .unwrap()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["Three stars", "Five stars", "Empty"]
        );
        assert_eq!(
            lib.ordered_playlists("", CollectionOrder::Recent).unwrap()[0].name,
            "Three stars"
        );
    }
    #[test]
    fn equally_named_artist_entities_are_ranked_using_their_own_tracks() {
        let mut lib = Library::open_in_memory().unwrap();
        for title in ["First", "Second"] {
            lib.import_scanned(&[ScannedTrack {
                path: format!("/{title}"),
                metadata: Metadata {
                    title: title.into(),
                    artist: Some("Same name".into()),
                    ..Default::default()
                },
            }])
            .unwrap();
        }
        let tracks = lib.ordered_tracks("", CollectionOrder::AddedDate).unwrap();
        let second = tracks
            .iter()
            .find(|t| t.metadata.title == "Second")
            .unwrap();
        lib.set_rating(&second.id.0, Some(5)).unwrap();
        let ranked = lib.ordered_artists("", CollectionOrder::Rating).unwrap();
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            lib.storage.artist_tracks(&ranked[0].id).unwrap()[0]
                .metadata
                .title,
            "Second"
        );
    }
    #[test]
    fn aggregate_added_and_rating_use_latest_member_and_only_rated_tracks() {
        let mut a = Track {
            id: weave_player_core::TrackId("a".into()),
            metadata: Metadata::default(),
            tags: vec![],
            rating: Some(4),
            added_at: 10,
        };
        let b = Track {
            id: weave_player_core::TrackId("b".into()),
            metadata: Metadata::default(),
            tags: vec![],
            rating: None,
            added_at: 20,
        };
        let rank = Rank::of(&[a.clone(), b], &HashMap::new());
        assert_eq!(rank.added, Some(20));
        assert_eq!(rank.rating, Some(4.0));
        a.added_at = 0;
        assert_eq!(Rank::of(&[a], &HashMap::new()).added, None);
    }
}
