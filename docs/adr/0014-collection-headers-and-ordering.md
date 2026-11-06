# Shared collection headers and ordering

All tracks, artists, albums and playlists use Weave's MediaHeader composition and
the existing page background wash, matching detail pages. Search is a window-header action that replaces the current media header inline
with the toolkit search bar. It filters the current collection through engine query
semantics, including playlist sources, and never opens a dialog or bottom sheet. Play sits at the far left of each
page control row; sort and the other secondary actions are grouped on the right. Album and
artist items retain their flat horizontal presentation.

CollectionOrder belongs to core; the engine ranks both track lists and whole
collections. Recent sorts by the latest recorded member playback, then latest
import when playback is unavailable. Release date uses the newest known member
year; added date uses the most recent member import; rating averages rated members
only. Newest/highest sorts first, unknown values last, and tied groups retain stable
engine order. Empty playlists remain visible. These describe member tracks, not
invented playlist creation or release timestamps. Artist membership follows entity
IDs, keeping separately imported artists with identical names distinct.

Home's recent artist/album/playlist sections use the same ordering as All pages.
Each All page remembers its selected order during the session. Manual column
sorting can override a selected order; selecting an order clears that override.
Detail-page sorts change presentation and the next playback order, and never rewrite
a manual playlist or saved filter. Opening another detail page resets this temporary order.

Jam uses the same four choices as a session-only playback rule, separate from
FilterSpec. Temporary ordering is applied after saved membership rules and limits,
so it cannot change which tracks match or enter a saved Jam. Sort precedes the
icon-only save action, which retains its tooltip and saved/disabled states. Starting
playback with an explicitly chosen order disables random shuffle for that queue;
selecting Shuffle afterward clears the temporary order. Saved album/playlist shuffle
preferences remain intact. All tracks exposes the player's shuffle action; its
default Recent display order does not override Shuffle until Sort is explicitly used.

No schema migration or new dependency is required. Native and visual verification
remain with the user; automated coverage checks metadata/history ranking, missing
values, separate same-name artists, search retention and playlist order preservation.

Contextual text search intersects engine matches with the current page's candidates,
preserving manual order and pre-existing limits. Jam's transient search is additional
to its saved query, so typing cannot replace saved query text or select new tracks
before a saved limit. Closing search restores that base collection.

Sort selections cycle descending → ascending → deselected when reselected. Sorting
is performed in the engine with missing values last in both directions, rather
than reversing the entire result. Deselecting restores normal engine/manual order.
Jam stores the temporary order and direction only in App state. Saved FilterSpec.sort
values from the engine/CLI remain unchanged, including on deselection.
