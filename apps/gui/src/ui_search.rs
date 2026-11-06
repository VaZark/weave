//! Inline search over the current page's existing collection.
use crate::{App, Dest, Element, Message};
use iced_adaptive_design::prelude::{self as w, surface};
use weave_player_lib::core::FilterSpec;

impl App {
    pub fn can_search_page(&self) -> bool {
        !(self.queue_open || self.save_manual && self.save_name.is_some())
            && self.playlist_edit.is_none()
            && (self.jam_open
                || matches!(
                    self.dest,
                    Dest::Tracks
                        | Dest::Artists
                        | Dest::Albums
                        | Dest::Playlists
                        | Dest::Recent
                        | Dest::Rediscover
                        | Dest::Artist(_)
                        | Dest::Album(_)
                        | Dest::Playlist(_)
                ))
    }
    pub fn page_search_bar(&self) -> Element<'_> {
        surface::toolbar(
            w::FocusScope::new(
                w::search_bar(&self.collection_query, Message::SearchText)
                    .placeholder(format!("Search {}", self.page_title()))
                    .on_close(Message::SearchOpen(false))
                    .id("page-search"),
            )
            .trap(false)
            .initial_focus("page-search"),
        )
    }
    /// Extra search conditions; the engine also intersects these with the page's candidate IDs.
    pub fn context_search_spec(&self) -> FilterSpec {
        let mut spec = FilterSpec {
            query: Some(self.collection_query.clone()),
            ..Default::default()
        };
        if self.jam_open {
            spec.sources = self.filter.to_spec().sources;
        } else {
            match self.dest {
                Dest::Playlist(i) => {
                    if let Some(playlist) = self.lib.playlists.get(i) {
                        spec.sources.push(playlist.name.clone());
                    }
                }
                Dest::Artist(i) => {
                    if let Some((artist, _)) = self.lib.artists().get(i) {
                        spec.artist_any.push(artist.clone());
                    }
                }
                Dest::Album(i) => {
                    if let Some((album, _, _)) = self.lib.albums().get(i) {
                        spec.album_any.push(album.clone());
                        // Candidate IDs already identify this album/artist pair, including unknown artists.
                    }
                }
                _ => {}
            }
        }
        spec
    }
}
